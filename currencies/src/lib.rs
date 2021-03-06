//! # Currencies Module
//!
//! ## Overview
//!
//! The currencies module provides a mixed currencies system, by configuring a native currency which implements
//! `BasicCurrencyExtended`, and a multi-currency which implements `MultiCurrency`.
//!
//! It also provides an adapter, to adapt `frame_support::traits::Currency` implementations into
//! `BasicCurrencyExtended`.
//!
//! The currencies module provides functionality of both `MultiCurrencyExtended` and `BasicCurrencyExtended`, via
//! unified interfaces, and all calls would be delegated to the underlying multi-currency and base currency system.
//! A native currency ID could be set by `Trait::GetNativeCurrencyId`, to identify the native currency.
//!
//! ### Implementations
//!
//! The currencies module provides implementations for following traits.
//!
//! - `MultiCurrency` - Abstraction over a fungible multi-currency system.
//! - `MultiCurrencyExtended` - Extended `MultiCurrency` with additional helper types and methods, like updating balance
//! by a given signed integer amount.
//!
//! ## Interface
//!
//! ### Dispatchable Functions
//!
//! - `transfer` - Transfer some balance to another account, in a given currency.
//! - `transfer_native_currency` - Transfer some balance to another account, in native currency set in
//! `Trait::NativeCurrency`.
//! - `update_balance` - Update balance by signed integer amount, in a given currency, root origin required.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
	decl_error, decl_event, decl_module, decl_storage,
	traits::{
		Currency as PalletCurrency, ExistenceRequirement, Get, LockableCurrency as PalletLockableCurrency,
		ReservableCurrency as PalletReservableCurrency, WithdrawReason,
	},
};
use rstd::{convert::TryInto, marker};
use sp_runtime::{
	traits::{CheckedSub, StaticLookup, Zero},
	DispatchError, DispatchResult,
};
// FIXME: `pallet/frame-` prefix should be used for all pallet modules, but currently `frame_system`
// would cause compiling error in `decl_module!` and `construct_runtime!`
// #3295 https://github.com/paritytech/substrate/issues/3295
use frame_system::{self as system, ensure_root, ensure_signed};

use orml_traits::{
	arithmetic::Signed, BalanceStatus, BasicCurrency, BasicCurrencyExtended, BasicLockableCurrency,
	BasicReservableCurrency, LockIdentifier, MultiCurrency, MultiCurrencyExtended, MultiLockableCurrency,
	MultiReservableCurrency,
};

mod mock;
mod tests;

type BalanceOf<T> = <<T as Trait>::MultiCurrency as MultiCurrency<<T as frame_system::Trait>::AccountId>>::Balance;
type CurrencyIdOf<T> =
	<<T as Trait>::MultiCurrency as MultiCurrency<<T as frame_system::Trait>::AccountId>>::CurrencyId;

type AmountOf<T> =
	<<T as Trait>::MultiCurrency as MultiCurrencyExtended<<T as frame_system::Trait>::AccountId>>::Amount;

pub trait Trait: frame_system::Trait {
	type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;
	type MultiCurrency: MultiCurrencyExtended<Self::AccountId>
		+ MultiLockableCurrency<Self::AccountId>
		+ MultiReservableCurrency<Self::AccountId>;
	type NativeCurrency: BasicCurrencyExtended<Self::AccountId, Balance = BalanceOf<Self>, Amount = AmountOf<Self>>
		+ BasicLockableCurrency<Self::AccountId, Balance = BalanceOf<Self>>
		+ BasicReservableCurrency<Self::AccountId, Balance = BalanceOf<Self>>;
	type GetNativeCurrencyId: Get<CurrencyIdOf<Self>>;
}

decl_storage! {
	trait Store for Module<T: Trait> as Currencies {}
}

decl_event!(
	pub enum Event<T> where
		<T as frame_system::Trait>::AccountId,
		Amount = AmountOf<T>,
		Balance = BalanceOf<T>,
		CurrencyId = CurrencyIdOf<T>
	{
		/// Currency transfer success (currency_id, from, to, amount)
		Transferred(CurrencyId, AccountId, AccountId, Balance),
		/// Update balance success (currency_id, who, amount)
		BalanceUpdated(CurrencyId, AccountId, Amount),
		/// Deposit success (currency_id, who, amount)
		Deposited(CurrencyId, AccountId, Balance),
		/// Withdraw success (currency_id, who, amount)
		Withdrawn(CurrencyId, AccountId, Balance),
	}
);

decl_error! {
	/// Error for currencies module.
	pub enum Error for Module<T: Trait> {
		AmountIntoBalanceFailed,
		BalanceTooLow,
	}
}

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		type Error = Error<T>;

		const NativeCurrencyId: CurrencyIdOf<T> = T::GetNativeCurrencyId::get();

		fn deposit_event() = default;

		/// Transfer some balance to another account.
		pub fn transfer(
			origin,
			dest: <T::Lookup as StaticLookup>::Source,
			currency_id: CurrencyIdOf<T>,
			#[compact] amount: BalanceOf<T>,
		) {
			let from = ensure_signed(origin)?;
			let to = T::Lookup::lookup(dest)?;
			<Self as MultiCurrency<T::AccountId>>::transfer(currency_id, &from, &to, amount)?;
		}

		/// Transfer native currency balance from one account to another.
		pub fn transfer_native_currency(
			origin,
			dest: <T::Lookup as StaticLookup>::Source,
			#[compact] amount: BalanceOf<T>,
		) {
			let from = ensure_signed(origin)?;
			let to = T::Lookup::lookup(dest)?;
			T::NativeCurrency::transfer(&from, &to, amount)?;

			Self::deposit_event(RawEvent::Transferred(T::GetNativeCurrencyId::get(), from, to, amount));
		}

		/// Update balance of an account. This is a root call.
		pub fn update_balance(
			origin,
			who: <T::Lookup as StaticLookup>::Source,
			currency_id: CurrencyIdOf<T>,
			amount: AmountOf<T>,
		) {
			ensure_root(origin)?;
			let dest = T::Lookup::lookup(who)?;
			<Self as MultiCurrencyExtended<T::AccountId>>::update_balance(currency_id, &dest, amount)?;
		}
	}
}

impl<T: Trait> Module<T> {}

impl<T: Trait> MultiCurrency<T::AccountId> for Module<T> {
	type CurrencyId = CurrencyIdOf<T>;
	type Balance = BalanceOf<T>;

	fn total_issuance(currency_id: Self::CurrencyId) -> Self::Balance {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::total_issuance()
		} else {
			T::MultiCurrency::total_issuance(currency_id)
		}
	}

	fn total_balance(currency_id: Self::CurrencyId, who: &T::AccountId) -> Self::Balance {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::total_balance(who)
		} else {
			T::MultiCurrency::total_balance(currency_id, who)
		}
	}

	fn free_balance(currency_id: Self::CurrencyId, who: &T::AccountId) -> Self::Balance {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::free_balance(who)
		} else {
			T::MultiCurrency::free_balance(currency_id, who)
		}
	}

	fn ensure_can_withdraw(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::ensure_can_withdraw(who, amount)
		} else {
			T::MultiCurrency::ensure_can_withdraw(currency_id, who, amount)
		}
	}

	fn transfer(
		currency_id: Self::CurrencyId,
		from: &T::AccountId,
		to: &T::AccountId,
		amount: Self::Balance,
	) -> DispatchResult {
		if amount.is_zero() {
			return Ok(());
		}
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::transfer(from, to, amount)?;
		} else {
			T::MultiCurrency::transfer(currency_id, from, to, amount)?;
		}
		Self::deposit_event(RawEvent::Transferred(currency_id, from.clone(), to.clone(), amount));
		Ok(())
	}

	fn deposit(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		if amount.is_zero() {
			return Ok(());
		}
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::deposit(who, amount)?;
		} else {
			T::MultiCurrency::deposit(currency_id, who, amount)?;
		}
		Self::deposit_event(RawEvent::Deposited(currency_id, who.clone(), amount));
		Ok(())
	}

	fn withdraw(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		if amount.is_zero() {
			return Ok(());
		}
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::withdraw(who, amount)?;
		} else {
			T::MultiCurrency::withdraw(currency_id, who, amount)?;
		}
		Self::deposit_event(RawEvent::Withdrawn(currency_id, who.clone(), amount));
		Ok(())
	}

	fn can_slash(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> bool {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::can_slash(who, amount)
		} else {
			T::MultiCurrency::can_slash(currency_id, who, amount)
		}
	}

	fn slash(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> Self::Balance {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::slash(who, amount)
		} else {
			T::MultiCurrency::slash(currency_id, who, amount)
		}
	}
}

impl<T: Trait> MultiCurrencyExtended<T::AccountId> for Module<T> {
	type Amount = AmountOf<T>;

	fn update_balance(currency_id: Self::CurrencyId, who: &T::AccountId, by_amount: Self::Amount) -> DispatchResult {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::update_balance(who, by_amount)?;
		} else {
			T::MultiCurrency::update_balance(currency_id, who, by_amount)?;
		}
		Self::deposit_event(RawEvent::BalanceUpdated(currency_id, who.clone(), by_amount));
		Ok(())
	}
}

impl<T: Trait> MultiLockableCurrency<T::AccountId> for Module<T> {
	type Moment = T::BlockNumber;

	fn set_lock(lock_id: LockIdentifier, currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::set_lock(lock_id, who, amount);
		} else {
			T::MultiCurrency::set_lock(lock_id, currency_id, who, amount);
		}
	}

	fn extend_lock(lock_id: LockIdentifier, currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::extend_lock(lock_id, who, amount);
		} else {
			T::MultiCurrency::extend_lock(lock_id, currency_id, who, amount);
		}
	}

	fn remove_lock(lock_id: LockIdentifier, currency_id: Self::CurrencyId, who: &T::AccountId) {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::remove_lock(lock_id, who);
		} else {
			T::MultiCurrency::remove_lock(lock_id, currency_id, who);
		}
	}
}

impl<T: Trait> MultiReservableCurrency<T::AccountId> for Module<T> {
	fn can_reserve(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> bool {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::can_reserve(who, value)
		} else {
			T::MultiCurrency::can_reserve(currency_id, who, value)
		}
	}

	fn slash_reserved(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> Self::Balance {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::slash_reserved(who, value)
		} else {
			T::MultiCurrency::slash_reserved(currency_id, who, value)
		}
	}

	fn reserved_balance(currency_id: Self::CurrencyId, who: &T::AccountId) -> Self::Balance {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::reserved_balance(who)
		} else {
			T::MultiCurrency::reserved_balance(currency_id, who)
		}
	}

	fn reserve(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> DispatchResult {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::reserve(who, value)
		} else {
			T::MultiCurrency::reserve(currency_id, who, value)
		}
	}

	fn unreserve(currency_id: Self::CurrencyId, who: &T::AccountId, value: Self::Balance) -> Self::Balance {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::unreserve(who, value)
		} else {
			T::MultiCurrency::unreserve(currency_id, who, value)
		}
	}

	fn repatriate_reserved(
		currency_id: Self::CurrencyId,
		slashed: &T::AccountId,
		beneficiary: &T::AccountId,
		value: Self::Balance,
		status: BalanceStatus,
	) -> rstd::result::Result<Self::Balance, DispatchError> {
		if currency_id == T::GetNativeCurrencyId::get() {
			T::NativeCurrency::repatriate_reserved(slashed, beneficiary, value, status)
		} else {
			T::MultiCurrency::repatriate_reserved(currency_id, slashed, beneficiary, value, status)
		}
	}
}

pub struct Currency<T, GetCurrencyId>(marker::PhantomData<T>, marker::PhantomData<GetCurrencyId>);

impl<T, GetCurrencyId> BasicCurrency<T::AccountId> for Currency<T, GetCurrencyId>
where
	T: Trait,
	GetCurrencyId: Get<CurrencyIdOf<T>>,
{
	type Balance = BalanceOf<T>;

	fn total_issuance() -> Self::Balance {
		<Module<T>>::total_issuance(GetCurrencyId::get())
	}

	fn total_balance(who: &T::AccountId) -> Self::Balance {
		<Module<T>>::total_balance(GetCurrencyId::get(), who)
	}

	fn free_balance(who: &T::AccountId) -> Self::Balance {
		<Module<T>>::free_balance(GetCurrencyId::get(), who)
	}

	fn ensure_can_withdraw(who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		<Module<T>>::ensure_can_withdraw(GetCurrencyId::get(), who, amount)
	}

	fn transfer(from: &T::AccountId, to: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		<Module<T> as MultiCurrency<T::AccountId>>::transfer(GetCurrencyId::get(), from, to, amount)
	}

	fn deposit(who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		<Module<T>>::deposit(GetCurrencyId::get(), who, amount)
	}

	fn withdraw(who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
		<Module<T>>::withdraw(GetCurrencyId::get(), who, amount)
	}

	fn can_slash(who: &T::AccountId, amount: Self::Balance) -> bool {
		<Module<T>>::can_slash(GetCurrencyId::get(), who, amount)
	}

	fn slash(who: &T::AccountId, amount: Self::Balance) -> Self::Balance {
		<Module<T>>::slash(GetCurrencyId::get(), who, amount)
	}
}

impl<T, GetCurrencyId> BasicCurrencyExtended<T::AccountId> for Currency<T, GetCurrencyId>
where
	T: Trait,
	GetCurrencyId: Get<CurrencyIdOf<T>>,
{
	type Amount = AmountOf<T>;

	fn update_balance(who: &T::AccountId, by_amount: Self::Amount) -> DispatchResult {
		<Module<T> as MultiCurrencyExtended<T::AccountId>>::update_balance(GetCurrencyId::get(), who, by_amount)
	}
}

impl<T, GetCurrencyId> BasicLockableCurrency<T::AccountId> for Currency<T, GetCurrencyId>
where
	T: Trait,
	GetCurrencyId: Get<CurrencyIdOf<T>>,
{
	type Moment = T::BlockNumber;

	fn set_lock(lock_id: LockIdentifier, who: &T::AccountId, amount: Self::Balance) {
		<Module<T> as MultiLockableCurrency<T::AccountId>>::set_lock(lock_id, GetCurrencyId::get(), who, amount);
	}

	fn extend_lock(lock_id: LockIdentifier, who: &T::AccountId, amount: Self::Balance) {
		<Module<T> as MultiLockableCurrency<T::AccountId>>::extend_lock(lock_id, GetCurrencyId::get(), who, amount);
	}

	fn remove_lock(lock_id: LockIdentifier, who: &T::AccountId) {
		<Module<T> as MultiLockableCurrency<T::AccountId>>::remove_lock(lock_id, GetCurrencyId::get(), who);
	}
}

impl<T, GetCurrencyId> BasicReservableCurrency<T::AccountId> for Currency<T, GetCurrencyId>
where
	T: Trait,
	GetCurrencyId: Get<CurrencyIdOf<T>>,
{
	fn can_reserve(who: &T::AccountId, value: Self::Balance) -> bool {
		<Module<T> as MultiReservableCurrency<T::AccountId>>::can_reserve(GetCurrencyId::get(), who, value)
	}

	fn slash_reserved(who: &T::AccountId, value: Self::Balance) -> Self::Balance {
		<Module<T> as MultiReservableCurrency<T::AccountId>>::slash_reserved(GetCurrencyId::get(), who, value)
	}

	fn reserved_balance(who: &T::AccountId) -> Self::Balance {
		<Module<T> as MultiReservableCurrency<T::AccountId>>::reserved_balance(GetCurrencyId::get(), who)
	}

	fn reserve(who: &T::AccountId, value: Self::Balance) -> DispatchResult {
		<Module<T> as MultiReservableCurrency<T::AccountId>>::reserve(GetCurrencyId::get(), who, value)
	}

	fn unreserve(who: &T::AccountId, value: Self::Balance) -> Self::Balance {
		<Module<T> as MultiReservableCurrency<T::AccountId>>::unreserve(GetCurrencyId::get(), who, value)
	}

	fn repatriate_reserved(
		slashed: &T::AccountId,
		beneficiary: &T::AccountId,
		value: Self::Balance,
		status: BalanceStatus,
	) -> rstd::result::Result<Self::Balance, DispatchError> {
		<Module<T> as MultiReservableCurrency<T::AccountId>>::repatriate_reserved(
			GetCurrencyId::get(),
			slashed,
			beneficiary,
			value,
			status,
		)
	}
}

pub type NativeCurrencyOf<T> = Currency<T, <T as Trait>::GetNativeCurrencyId>;

/// Adapt other currency traits implementation to `BasicCurrency`.
pub struct BasicCurrencyAdapter<T, Currency, BalanceConvert>(marker::PhantomData<(T, Currency, BalanceConvert)>);

type PalletBalanceOf<A, Currency> = <Currency as PalletCurrency<A>>::Balance;

// Adapt `frame_support::traits::Currency`
impl<AccountId, T, Currency, BalanceConvert> BasicCurrency<AccountId>
	for BasicCurrencyAdapter<T, Currency, BalanceConvert>
where
	T: Trait,
	Currency: PalletCurrency<AccountId>,
	BalanceConvert: From<PalletBalanceOf<AccountId, Currency>>
		+ Into<PalletBalanceOf<AccountId, Currency>>
		+ From<BalanceOf<T>>
		+ Into<BalanceOf<T>>,
{
	type Balance = BalanceOf<T>;

	fn total_issuance() -> Self::Balance {
		BalanceConvert::from(Currency::total_issuance()).into()
	}

	fn total_balance(who: &AccountId) -> Self::Balance {
		BalanceConvert::from(Currency::total_balance(who)).into()
	}

	fn free_balance(who: &AccountId) -> Self::Balance {
		BalanceConvert::from(Currency::free_balance(who)).into()
	}

	fn ensure_can_withdraw(who: &AccountId, amount: Self::Balance) -> DispatchResult {
		let new_balance_pallet = {
			let new_balance = Self::free_balance(who)
				.checked_sub(&amount)
				.ok_or(Error::<T>::BalanceTooLow)?;
			BalanceConvert::from(new_balance).into()
		};
		let amount_pallet = BalanceConvert::from(amount).into();
		Currency::ensure_can_withdraw(who, amount_pallet, WithdrawReason::Transfer.into(), new_balance_pallet)
	}

	fn transfer(from: &AccountId, to: &AccountId, amount: Self::Balance) -> DispatchResult {
		let amount_pallet = BalanceConvert::from(amount).into();
		Currency::transfer(from, to, amount_pallet, ExistenceRequirement::AllowDeath)
	}

	fn deposit(who: &AccountId, amount: Self::Balance) -> DispatchResult {
		let _ = Currency::deposit_creating(who, BalanceConvert::from(amount).into());
		Ok(())
	}

	fn withdraw(who: &AccountId, amount: Self::Balance) -> DispatchResult {
		Currency::withdraw(
			who,
			BalanceConvert::from(amount).into(),
			WithdrawReason::Transfer.into(),
			ExistenceRequirement::AllowDeath,
		)
		.map(|_| ())
	}

	fn can_slash(who: &AccountId, amount: Self::Balance) -> bool {
		Currency::can_slash(who, BalanceConvert::from(amount).into())
	}

	fn slash(who: &AccountId, amount: Self::Balance) -> Self::Balance {
		let (_, gap) = Currency::slash(who, BalanceConvert::from(amount).into());
		BalanceConvert::from(gap).into()
	}
}

// Adapt `frame_support::traits::Currency`
impl<AccountId, T, Currency, BalanceConvert> BasicCurrencyExtended<AccountId>
	for BasicCurrencyAdapter<T, Currency, BalanceConvert>
where
	T: Trait,
	Currency: PalletCurrency<AccountId>,
	BalanceConvert: From<PalletBalanceOf<AccountId, Currency>>
		+ Into<PalletBalanceOf<AccountId, Currency>>
		+ From<BalanceOf<T>>
		+ Into<BalanceOf<T>>,
{
	type Amount = AmountOf<T>;

	fn update_balance(who: &AccountId, by_amount: Self::Amount) -> DispatchResult {
		let by_balance = by_amount
			.abs()
			.try_into()
			.map_err(|_| Error::<T>::AmountIntoBalanceFailed)?;
		if by_amount.is_positive() {
			Self::deposit(who, by_balance)
		} else {
			Self::withdraw(who, by_balance)
		}
	}
}

// Adapt `frame_support::traits::LockableCurrency`
impl<AccountId, T, Currency, BalanceConvert> BasicLockableCurrency<AccountId>
	for BasicCurrencyAdapter<T, Currency, BalanceConvert>
where
	T: Trait,
	Currency: PalletLockableCurrency<AccountId>,
	BalanceConvert: From<PalletBalanceOf<AccountId, Currency>>
		+ Into<PalletBalanceOf<AccountId, Currency>>
		+ From<BalanceOf<T>>
		+ Into<BalanceOf<T>>,
{
	type Moment = T::BlockNumber;

	fn set_lock(lock_id: LockIdentifier, who: &AccountId, amount: Self::Balance) {
		Currency::set_lock(
			lock_id.into(),
			who,
			BalanceConvert::from(amount).into(),
			(WithdrawReason::Transfer | WithdrawReason::Reserve).into(),
		);
	}

	fn extend_lock(lock_id: LockIdentifier, who: &AccountId, amount: Self::Balance) {
		Currency::extend_lock(
			lock_id.into(),
			who,
			BalanceConvert::from(amount).into(),
			(WithdrawReason::Transfer | WithdrawReason::Reserve).into(),
		);
	}

	fn remove_lock(lock_id: LockIdentifier, who: &AccountId) {
		Currency::remove_lock(lock_id.into(), who);
	}
}

// Adapt `frame_support::traits::ReservableCurrency`
impl<AccountId, T, Currency, BalanceConvert> BasicReservableCurrency<AccountId>
	for BasicCurrencyAdapter<T, Currency, BalanceConvert>
where
	T: Trait,
	Currency: PalletReservableCurrency<AccountId>,
	BalanceConvert: From<PalletBalanceOf<AccountId, Currency>>
		+ Into<PalletBalanceOf<AccountId, Currency>>
		+ From<BalanceOf<T>>
		+ Into<BalanceOf<T>>,
{
	fn can_reserve(who: &AccountId, value: Self::Balance) -> bool {
		Currency::can_reserve(who, BalanceConvert::from(value).into())
	}

	fn slash_reserved(who: &AccountId, value: Self::Balance) -> Self::Balance {
		let (_, gap) = Currency::slash_reserved(who, BalanceConvert::from(value).into());
		BalanceConvert::from(gap).into()
	}

	fn reserved_balance(who: &AccountId) -> Self::Balance {
		BalanceConvert::from(Currency::reserved_balance(who)).into()
	}

	fn reserve(who: &AccountId, value: Self::Balance) -> DispatchResult {
		Currency::reserve(who, BalanceConvert::from(value).into())
	}

	fn unreserve(who: &AccountId, value: Self::Balance) -> Self::Balance {
		BalanceConvert::from(Currency::unreserve(who, BalanceConvert::from(value).into())).into()
	}

	fn repatriate_reserved(
		slashed: &AccountId,
		beneficiary: &AccountId,
		value: Self::Balance,
		status: BalanceStatus,
	) -> rstd::result::Result<Self::Balance, DispatchError> {
		Currency::repatriate_reserved(slashed, beneficiary, BalanceConvert::from(value).into(), status.into())
			.map(|a| BalanceConvert::from(a).into())
	}
}
