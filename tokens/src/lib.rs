//! # Tokens Module
//!
//! ## Overview
//!
//! The tokens module provides fungible multi-currency functionality that implements `MultiCurrency` trait.
//!
//! The tokens module provides functions for:
//!
//! - Querying and setting the balance of a given account.
//! - Getting and managing total issuance.
//! - Balance transfer between accounts.
//! - Depositing and withdrawing balance.
//! - Slashing an account balance.
//!
//! ### Implementations
//!
//! The tokens module provides implementations for following traits.
//!
//! - `MultiCurrency` - Abstraction over a fungible multi-currency system.
//! - `MultiCurrencyExtended` - Extended `MultiCurrency` with additional helper types and methods, like updating balance
//! by a given signed integer amount.
//!
//! ## Interface
//!
//! ### Dispatchable Functions
//!
//! - `transfer` - Transfer some balance to another account.
//! - `transfer_all` - Transfer all balance to another account.
//!
//! ### Genesis Config
//!
//! The tokens module depends on the `GenesisConfig`. Endowed accounts could be configured in genesis configs.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use frame_support::{decl_error, decl_event, decl_module, decl_storage, ensure, traits::Get, Parameter};
use rstd::convert::{TryFrom, TryInto};
use rstd::prelude::*;
use sp_runtime::{
	traits::{AtLeast32Bit, CheckedAdd, CheckedSub, MaybeSerializeDeserialize, Member, Saturating, StaticLookup, Zero},
	DispatchError, DispatchResult, RuntimeDebug,
};
// FIXME: `pallet/frame-` prefix should be used for all pallet modules, but currently `frame_system`
// would cause compiling error in `decl_module!` and `construct_runtime!`
// #3295 https://github.com/paritytech/substrate/issues/3295
use frame_system::{self as system, ensure_signed};

#[cfg(feature = "std")]
use rstd::collections::btree_map::BTreeMap;

use orml_traits::{
	arithmetic::{self, Signed},
	BalanceStatus, LockIdentifier, MultiCurrency, MultiCurrencyExtended, MultiLockableCurrency,
	MultiReservableCurrency, OnDustRemoval,
};

mod mock;
mod tests;

pub trait Trait: frame_system::Trait {
	type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;
	type Balance: Parameter + Member + AtLeast32Bit + Default + Copy + MaybeSerializeDeserialize;
	type Amount: Signed
		+ TryInto<Self::Balance>
		+ TryFrom<Self::Balance>
		+ Parameter
		+ Member
		+ arithmetic::SimpleArithmetic
		+ Default
		+ Copy
		+ MaybeSerializeDeserialize;
	type CurrencyId: Parameter + Member + Copy + MaybeSerializeDeserialize + Ord;
	type ExistentialDeposit: Get<Self::Balance>;
	type DustRemoval: OnDustRemoval<Self::Balance>;
}

/// A single lock on a balance. There can be many of these on an account and they "overlap", so the
/// same balance is frozen by multiple locks.
#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
pub struct BalanceLock<Balance> {
	/// An identifier for this lock. Only one lock may be in existence for each identifier.
	pub id: LockIdentifier,
	/// The amount which the free balance may not drop below when this lock is in effect.
	pub amount: Balance,
}

/// balance information for an account.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Default, RuntimeDebug)]
pub struct AccountData<Balance> {
	/// Non-reserved part of the balance. There may still be restrictions on this, but it is the
	/// total pool what may in principle be transferred, reserved.
	///
	/// This is the only balance that matters in terms of most operations on tokens.
	pub free: Balance,
	/// Balance which is reserved and may not be used at all.
	///
	/// This can still get slashed, but gets slashed last of all.
	///
	/// This balance is a 'reserve' balance that other subsystems use in order to set aside tokens
	/// that are still 'owned' by the account holder, but which are suspendable.
	pub reserved: Balance,
	/// The amount that `free` may not drop below when withdrawing.
	pub frozen: Balance,
}

impl<Balance: Saturating + Copy + Ord> AccountData<Balance> {
	/// The amount that this account's free balance may not be reduced beyond.
	fn frozen(&self) -> Balance {
		self.frozen
	}
	/// The total balance in this account including any that is reserved and ignoring any frozen.
	fn total(&self) -> Balance {
		self.free.saturating_add(self.reserved)
	}
}

decl_storage! {
	trait Store for Module<T: Trait> as Tokens {
		/// The total issuance of a token type.
		pub TotalIssuance get(fn total_issuance) build(|config: &GenesisConfig<T>| {
			config
				.endowed_accounts
				.iter()
				.map(|(_, currency_id, initial_balance)| (currency_id, initial_balance))
				.fold(BTreeMap::<T::CurrencyId, T::Balance>::new(), |mut acc, (currency_id, initial_balance)| {
					if let Some(issuance) = acc.get_mut(currency_id) {
						*issuance = issuance.checked_add(initial_balance).expect("total issuance cannot overflow when building genesis");
					} else {
						acc.insert(*currency_id, *initial_balance);
					}
					acc
				})
				.into_iter()
				.collect::<Vec<_>>()
		}): map hasher(twox_64_concat) T::CurrencyId => T::Balance;

		/// Any liquidity locks of a token type under an account.
		/// NOTE: Should only be accessed when setting, changing and freeing a lock.
		pub Locks get(fn locks): double_map hasher(twox_64_concat) T::CurrencyId, hasher(blake2_128_concat) T::AccountId => Vec<BalanceLock<T::Balance>>;

		/// The balance of a token type under an account.
		///
		/// NOTE: If the total is ever zero, decrease account ref account.
		///
		/// NOTE: This is only used in the case that this module is used to store balances.
		pub Accounts get(fn accounts): double_map hasher(twox_64_concat) T::CurrencyId, hasher(blake2_128_concat) T::AccountId => AccountData<T::Balance>;
	}
	add_extra_genesis {
		config(endowed_accounts): Vec<(T::AccountId, T::CurrencyId, T::Balance)>;

		build(|config: &GenesisConfig<T>| {
			config.endowed_accounts.iter().for_each(|(account_id, currency_id, initial_balance)| {
				<Accounts<T>>::mutate(currency_id, account_id, |account_data| account_data.free = *initial_balance)
			})
		})
	}
}

decl_event!(
	pub enum Event<T> where
		<T as frame_system::Trait>::AccountId,
		<T as Trait>::CurrencyId,
		<T as Trait>::Balance
	{
		/// Token transfer success (currency_id, from, to, amount)
		Transferred(CurrencyId, AccountId, AccountId, Balance),
	}
);

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		type Error = Error<T>;

		fn deposit_event() = default;

		/// Transfer some balance to another account.
		pub fn transfer(
			origin,
			dest: <T::Lookup as StaticLookup>::Source,
			currency_id: T::CurrencyId,
			#[compact] amount: T::Balance,
		) {
			let from = ensure_signed(origin)?;
			let to = T::Lookup::lookup(dest)?;
			<Self as MultiCurrency<_>>::transfer(currency_id, &from, &to, amount)?;

			Self::deposit_event(RawEvent::Transferred(currency_id, from, to, amount));
		}

		/// Transfer all remaining balance to the given account.
		pub fn transfer_all(
			origin,
			dest: <T::Lookup as StaticLookup>::Source,
			currency_id: T::CurrencyId,
		) {
			let from = ensure_signed(origin)?;
			let to = T::Lookup::lookup(dest)?;
			let balance = <Self as MultiCurrency<T::AccountId>>::free_balance(currency_id, &from);
			<Self as MultiCurrency<T::AccountId>>::transfer(currency_id, &from, &to, balance)?;

			Self::deposit_event(RawEvent::Transferred(currency_id, from, to, balance));
		}
	}
}

decl_error! {
	/// Error for token module.
	pub enum Error for Module<T: Trait> {
		BalanceTooLow,
		TotalIssuanceOverflow,
		AmountIntoBalanceFailed,
		ExistentialDeposit,
		LiquidityRestrictions,
	}
}

impl<T: Trait> Module<T> {
	/// Set free balance of `who` to a new value, meanwhile enforce existential rule.
	///
	/// Note this will not maintain total issuance except balance is less to ExistentialDeposit,
	/// and the caller is expected to do it.
	fn set_free_balance(currency_id: T::CurrencyId, who: &T::AccountId, balance: T::Balance) {
		if balance < T::ExistentialDeposit::get() {
			<Accounts<T>>::mutate(currency_id, who, |account_data| account_data.free = Zero::zero());
			T::DustRemoval::on_dust_removal(balance);
			<TotalIssuance<T>>::mutate(currency_id, |v| *v -= balance);
		} else {
			<Accounts<T>>::mutate(currency_id, who, |account_data| account_data.free = balance);
		}
	}

	/// Set reserved balance of `who` to a new value, meanwhile enforce existential rule.
	///
	/// Note this will not maintain total issuance, and the caller is expected to do it.
	fn set_reserved_balance(currency_id: T::CurrencyId, who: &T::AccountId, balance: T::Balance) {
		<Accounts<T>>::mutate(currency_id, who, |account_data| account_data.reserved = balance);
	}

	/// Update the account entry for `who` under `currency_id`, given the locks.
	fn update_locks(currency_id: T::CurrencyId, who: &T::AccountId, locks: &[BalanceLock<T::Balance>]) {
		// update account data
		<Accounts<T>>::mutate(currency_id, who, |account_data| {
			account_data.frozen = Zero::zero();
			for lock in locks.iter() {
				account_data.frozen = account_data.frozen.max(lock.amount);
			}
		});

		// update locks
		let existed = <Locks<T>>::contains_key(currency_id, who);
		if locks.is_empty() {
			<Locks<T>>::remove(currency_id, who);
			if existed {
				// decrease account ref count when destruct lock
				system::Module::<T>::dec_ref(who);
			}
		} else {
			<Locks<T>>::insert(currency_id, who, locks);
			if !existed {
				// increase account ref count when initialize lock
				system::Module::<T>::inc_ref(who);
			}
		}
	}
}

impl<T: Trait> MultiCurrency<T::AccountId> for Module<T> {
	type CurrencyId = T::CurrencyId;
	type Balance = T::Balance;

	fn total_issuance(currency_id: Self::CurrencyId) -> Self::Balance {
		<TotalIssuance<T>>::get(currency_id)
	}

	fn total_balance(currency_id: Self::CurrencyId, who: &T::AccountId) -> Self::Balance {
		Self::accounts(currency_id, who).total()
	}

	fn free_balance(currency_id: Self::CurrencyId, who: &T::AccountId) -> Self::Balance {
		Self::accounts(currency_id, who).free
	}

	fn ensure_can_withdraw(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		if amount.is_zero() {
			return Ok(());
		}

		let new_balance = Self::free_balance(currency_id, who)
			.checked_sub(&amount)
			.ok_or(Error::<T>::BalanceTooLow)?;
		ensure!(
			new_balance >= Self::accounts(currency_id, who).frozen(),
			Error::<T>::LiquidityRestrictions
		);
		Ok(())
	}

	fn transfer(
		currency_id: Self::CurrencyId,
		from: &T::AccountId,
		to: &T::AccountId,
		amount: Self::Balance,
	) -> DispatchResult {
		if amount.is_zero() || from == to {
			return Ok(());
		}
		Self::ensure_can_withdraw(currency_id, from, amount)?;

		let from_balance = Self::free_balance(currency_id, from);
		let to_balance = Self::free_balance(currency_id, to);
		ensure!(
			to_balance + amount >= T::ExistentialDeposit::get(),
			Error::<T>::ExistentialDeposit,
		);

		if from != to {
			Self::set_free_balance(currency_id, from, from_balance - amount);
			Self::set_free_balance(currency_id, to, to_balance + amount);
		}

		Ok(())
	}

	fn deposit(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		if amount.is_zero() {
			return Ok(());
		}

		ensure!(
			Self::total_issuance(currency_id).checked_add(&amount).is_some(),
			Error::<T>::TotalIssuanceOverflow,
		);

		let balance = Self::free_balance(currency_id, who);
		// Nothing happens if deposition doesn't meet existential deposit rule,
		// consistent behavior with pallet-balances.
		if balance.is_zero() && amount < T::ExistentialDeposit::get() {
			return Ok(());
		}

		<TotalIssuance<T>>::mutate(currency_id, |v| *v += amount);
		Self::set_free_balance(currency_id, who, balance + amount);

		Ok(())
	}

	fn withdraw(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		if amount.is_zero() {
			return Ok(());
		}
		Self::ensure_can_withdraw(currency_id, who, amount)?;

		<TotalIssuance<T>>::mutate(currency_id, |v| *v -= amount);
		Self::set_free_balance(currency_id, who, Self::free_balance(currency_id, who) - amount);

		Ok(())
	}

	// Check if `value` amount of free balance can be slashed from `who`.
	fn can_slash(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> bool {
		if value.is_zero() {
			return true;
		}
		Self::free_balance(currency_id, who) >= value
	}

	/// Is a no-op if `value` to be slashed is zero.
	///
	/// NOTE: `slash()` prefers free balance, but assumes that reserve balance can be drawn
	/// from in extreme circumstances. `can_slash()` should be used prior to `slash()` to avoid having
	/// to draw from reserved funds, however we err on the side of punishment if things are inconsistent
	/// or `can_slash` wasn't used appropriately.
	fn slash(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> Self::Balance {
		if amount.is_zero() {
			return amount;
		}

		let account = Self::accounts(currency_id, who);
		let free_slashed_amount = account.free.min(amount);
		let mut remaining_slash = amount - free_slashed_amount;

		// slash free balance
		if !free_slashed_amount.is_zero() {
			Self::set_free_balance(currency_id, who, account.free - free_slashed_amount);
		}

		// slash reserved balance
		if !remaining_slash.is_zero() {
			let reserved_slashed_amount = account.reserved.min(remaining_slash);
			remaining_slash -= reserved_slashed_amount;
			Self::set_reserved_balance(currency_id, who, account.reserved - reserved_slashed_amount);
		}

		<TotalIssuance<T>>::mutate(currency_id, |v| *v -= amount - remaining_slash);
		remaining_slash
	}
}

impl<T: Trait> MultiCurrencyExtended<T::AccountId> for Module<T> {
	type Amount = T::Amount;

	fn update_balance(currency_id: Self::CurrencyId, who: &T::AccountId, by_amount: Self::Amount) -> DispatchResult {
		if by_amount.is_zero() {
			return Ok(());
		}

		let by_balance =
			TryInto::<Self::Balance>::try_into(by_amount.abs()).map_err(|_| Error::<T>::AmountIntoBalanceFailed)?;
		if by_amount.is_positive() {
			Self::deposit(currency_id, who, by_balance)
		} else {
			Self::withdraw(currency_id, who, by_balance)
		}
	}
}

impl<T: Trait> MultiLockableCurrency<T::AccountId> for Module<T> {
	type Moment = T::BlockNumber;

	// Set a lock on the balance of `who` under `currency_id`.
	// Is a no-op if lock amount is zero.
	fn set_lock(lock_id: LockIdentifier, currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) {
		if amount.is_zero() {
			return;
		}
		let mut new_lock = Some(BalanceLock {
			id: lock_id,
			amount: amount,
		});
		let mut locks = Self::locks(currency_id, who)
			.into_iter()
			.filter_map(|lock| {
				if lock.id == lock_id {
					new_lock.take()
				} else {
					Some(lock)
				}
			})
			.collect::<Vec<_>>();
		if let Some(lock) = new_lock {
			locks.push(lock)
		}
		Self::update_locks(currency_id, who, &locks[..]);
	}

	// Extend a lock on the balance of `who` under `currency_id`.
	// Is a no-op if lock amount is zero
	fn extend_lock(lock_id: LockIdentifier, currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) {
		if amount.is_zero() {
			return;
		}
		let mut new_lock = Some(BalanceLock {
			id: lock_id,
			amount: amount,
		});
		let mut locks = Self::locks(currency_id, who)
			.into_iter()
			.filter_map(|lock| {
				if lock.id == lock_id {
					new_lock.take().map(|nl| BalanceLock {
						id: lock.id,
						amount: lock.amount.max(nl.amount),
					})
				} else {
					Some(lock)
				}
			})
			.collect::<Vec<_>>();
		if let Some(lock) = new_lock {
			locks.push(lock)
		}
		Self::update_locks(currency_id, who, &locks[..]);
	}

	fn remove_lock(lock_id: LockIdentifier, currency_id: Self::CurrencyId, who: &T::AccountId) {
		let mut locks = Self::locks(currency_id, who);
		locks.retain(|lock| lock.id != lock_id);
		Self::update_locks(currency_id, who, &locks[..]);
	}
}

impl<T: Trait> MultiReservableCurrency<T::AccountId> for Module<T> {
	/// Check if `who` can reserve `value` from their free balance.
	///
	/// Always `true` if value to be reserved is zero.
	fn can_reserve(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> bool {
		if value.is_zero() {
			return true;
		}
		Self::ensure_can_withdraw(currency_id, who, value).is_ok()
	}

	/// Slash from reserved balance, returning any amount that was unable to be slashed.
	///
	/// Is a no-op if the value to be slashed is zero.
	fn slash_reserved(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> Self::Balance {
		if value.is_zero() {
			return Zero::zero();
		}

		let reserved_balance = Self::reserved_balance(currency_id, who);
		let actual = reserved_balance.min(value);
		Self::set_reserved_balance(currency_id, who, reserved_balance - actual);
		<TotalIssuance<T>>::mutate(currency_id, |v| *v -= actual);
		value - actual
	}

	fn reserved_balance(currency_id: Self::CurrencyId, who: &T::AccountId) -> Self::Balance {
		Self::accounts(currency_id, who).reserved
	}

	/// Move `value` from the free balance from `who` to their reserved balance.
	///
	/// Is a no-op if value to be reserved is zero.
	fn reserve(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> DispatchResult {
		if value.is_zero() {
			return Ok(());
		}
		Self::ensure_can_withdraw(currency_id, who, value)?;

		let account = Self::accounts(currency_id, who);
		Self::set_free_balance(currency_id, who, account.free - value);
		Self::set_reserved_balance(currency_id, who, account.reserved + value);
		Ok(())
	}

	/// Unreserve some funds, returning any amount that was unable to be unreserved.
	///
	/// Is a no-op if the value to be unreserved is zero.
	fn unreserve(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> Self::Balance {
		if value.is_zero() {
			return Zero::zero();
		}

		let account = Self::accounts(currency_id, who);
		let actual = account.reserved.min(value);
		Self::set_reserved_balance(currency_id, who, account.reserved - actual);
		Self::set_free_balance(currency_id, who, account.free + actual);
		value - actual
	}

	/// Move the reserved balance of one account into the balance of another, according to `status`.
	///
	/// Is a no-op if:
	/// - the value to be moved is zero; or
	/// - the `slashed` id equal to `beneficiary` and the `status` is `Reserved`.
	fn repatriate_reserved(
		currency_id: Self::CurrencyId,
		slashed: &T::AccountId,
		beneficiary: &T::AccountId,
		value: Self::Balance,
		status: BalanceStatus,
	) -> rstd::result::Result<Self::Balance, DispatchError> {
		if value.is_zero() {
			return Ok(Zero::zero());
		}

		if slashed == beneficiary {
			return match status {
				BalanceStatus::Free => Ok(Self::unreserve(currency_id, slashed, value)),
				BalanceStatus::Reserved => Ok(value.saturating_sub(Self::reserved_balance(currency_id, slashed))),
			};
		}

		let from_account = Self::accounts(currency_id, slashed);
		let to_account = Self::accounts(currency_id, beneficiary);
		let actual = from_account.reserved.min(value);
		match status {
			BalanceStatus::Free => {
				Self::set_free_balance(currency_id, beneficiary, to_account.free + actual);
			}
			BalanceStatus::Reserved => {
				Self::set_reserved_balance(currency_id, beneficiary, to_account.reserved + actual);
			}
		}
		Self::set_reserved_balance(currency_id, slashed, from_account.reserved - actual);
		Ok(value - actual)
	}
}
