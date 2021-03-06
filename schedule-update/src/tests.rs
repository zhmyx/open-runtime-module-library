//! Unit tests for the gradually-update module.

#![cfg(test)]

use super::*;
use frame_support::{assert_noop, assert_ok};
use mock::{BalancesCall, Call, ExtBuilder, Origin, Runtime, ScheduleUpdateModule, System, TestEvent};
use sp_runtime::traits::OnInitialize;

#[test]
fn schedule_dispatch_should_work() {
	ExtBuilder::default().build().execute_with(|| {
		// NormalDispatches
		let call = Call::Balances(BalancesCall::transfer(2, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(2, 0));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		// OperationalDispatches
		let call = Call::Balances(BalancesCall::set_balance(1, 10, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::ROOT,
			call,
			DelayedDispatchTime::After(3)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(4, 1));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));
	});
}

#[test]
fn schedule_dispatch_should_fail() {
	ExtBuilder::default().build().execute_with(|| {
		let call = Call::Balances(BalancesCall::transfer(2, 11));
		assert_noop!(
			ScheduleUpdateModule::schedule_dispatch(Origin::signed(1), call, DelayedDispatchTime::At(0)),
			Error::<Runtime>::InvalidDelayedDispatchTime
		);
	});
}

#[test]
fn cancel_deplayed_dispatch_should_work() {
	ExtBuilder::default().build().execute_with(|| {
		// NormalDispatches
		let call = Call::Balances(BalancesCall::transfer(2, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(2, 0));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		assert_ok!(ScheduleUpdateModule::cancel_deplayed_dispatch(Origin::signed(1), 2, 0));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::CancelDeplayedDispatch(0));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		// root cancel NormalDispatches
		let call = Call::Balances(BalancesCall::transfer(2, 12));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::After(3)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(4, 1));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		assert_ok!(ScheduleUpdateModule::cancel_deplayed_dispatch(Origin::ROOT, 4, 1));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::CancelDeplayedDispatch(1));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		// OperationalDispatches
		let call = Call::Balances(BalancesCall::set_balance(2, 10, 13));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::ROOT,
			call,
			DelayedDispatchTime::At(5)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(5, 2));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		assert_ok!(ScheduleUpdateModule::cancel_deplayed_dispatch(Origin::ROOT, 5, 2));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::CancelDeplayedDispatch(2));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));
	});
}

#[test]
fn cancel_deplayed_dispatch_should_fail() {
	ExtBuilder::default().build().execute_with(|| {
		assert_noop!(
			ScheduleUpdateModule::cancel_deplayed_dispatch(Origin::signed(1), 2, 0),
			Error::<Runtime>::DispatchNotExisted
		);

		// NormalDispatches
		let call = Call::Balances(BalancesCall::transfer(2, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(2, 0));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		assert_noop!(
			ScheduleUpdateModule::cancel_deplayed_dispatch(Origin::signed(2), 2, 0),
			Error::<Runtime>::NoPermission
		);

		// OperationalDispatches
		let call = Call::Balances(BalancesCall::set_balance(2, 10, 13));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::ROOT,
			call,
			DelayedDispatchTime::At(5)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(5, 1));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		assert_noop!(
			ScheduleUpdateModule::cancel_deplayed_dispatch(Origin::signed(2), 5, 1),
			Error::<Runtime>::NoPermission
		);
	});
}

#[test]
fn on_initialize_should_work() {
	ExtBuilder::default().build().execute_with(|| {
		// NormalDispatches
		let call = Call::Balances(BalancesCall::transfer(2, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		let call = Call::Balances(BalancesCall::transfer(2, 12));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(3)
		));

		assert_eq!(System::events().len(), 7);
		ScheduleUpdateModule::on_initialize(1);
		assert_eq!(System::events().len(), 7);

		ScheduleUpdateModule::on_initialize(2);
		println!("{:?}", System::events());
		assert_eq!(System::events().len(), 9);
		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchSuccess(2, 0));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		ScheduleUpdateModule::on_initialize(3);
		assert_eq!(System::events().len(), 11);
		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchSuccess(3, 1));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		// OperationalDispatches
		let call = Call::Balances(BalancesCall::set_balance(3, 10, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::ROOT,
			call,
			DelayedDispatchTime::After(10)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(11, 2));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		let call = Call::Balances(BalancesCall::set_balance(3, 20, 21));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::ROOT,
			call,
			DelayedDispatchTime::After(12)
		));

		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatch(13, 3));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		assert_eq!(System::events().len(), 13);
		ScheduleUpdateModule::on_initialize(10);
		assert_eq!(System::events().len(), 13);

		ScheduleUpdateModule::on_initialize(11);
		println!("{:?}", System::events());
		assert_eq!(System::events().len(), 15);
		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchSuccess(11, 2));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		ScheduleUpdateModule::on_initialize(13);
		assert_eq!(System::events().len(), 17);
		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchSuccess(13, 3));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));
	});
}

#[test]
fn on_initialize_should_fail() {
	ExtBuilder::default().build().execute_with(|| {
		// NormalDispatches balance not enough
		let call = Call::Balances(BalancesCall::transfer(2, 110));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		assert_eq!(System::events().len(), 6);
		ScheduleUpdateModule::on_initialize(1);
		assert_eq!(System::events().len(), 6);

		ScheduleUpdateModule::on_initialize(2);
		println!("{:?}", System::events());
		assert_eq!(System::events().len(), 7);
		//TODO hold the error
		let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchFail(
			0,
			DispatchError::Module {
				index: 0,
				error: 3,
				message: None,
			},
		));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));

		// OperationalDispatches not root
		let call = Call::Balances(BalancesCall::set_balance(3, 10, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::After(10)
		));

		assert_eq!(System::events().len(), 8);
		ScheduleUpdateModule::on_initialize(10);
		assert_eq!(System::events().len(), 8);

		ScheduleUpdateModule::on_initialize(11);
		println!("{:?}", System::events());
		assert_eq!(System::events().len(), 9);
		let schedule_dispatch_event =
			TestEvent::schedule_update(RawEvent::ScheduleDispatchFail(1, DispatchError::BadOrigin));
		assert!(System::events()
			.iter()
			.any(|record| record.event == schedule_dispatch_event));
	});
}

#[test]
fn on_initialize_weight_exceed() {
	ExtBuilder::default().build().execute_with(|| {
		// NormalDispatches
		let call = Call::Balances(BalancesCall::transfer(2, 11));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		let call = Call::Balances(BalancesCall::transfer(2, 12));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		let call = Call::Balances(BalancesCall::transfer(2, 13));
		assert_ok!(ScheduleUpdateModule::schedule_dispatch(
			Origin::signed(1),
			call,
			DelayedDispatchTime::At(2)
		));

		assert_eq!(System::events().len(), 8);
		ScheduleUpdateModule::on_initialize(1);
		assert_eq!(System::events().len(), 8);

		ScheduleUpdateModule::on_initialize(2);
		println!("{:?}", System::events());
		assert_eq!(System::events().len(), 12);
		// TODO on_initialize should be sorted
		//let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchSuccess(0, 2));
		//assert!(System::events().iter().any(|record| record.event == schedule_dispatch_event));

		//let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchSuccess(2, 2));
		//assert!(System::events().iter().any(|record| record.event == schedule_dispatch_event));

		ScheduleUpdateModule::on_initialize(3);
		assert_eq!(System::events().len(), 14);
		//let schedule_dispatch_event = TestEvent::schedule_update(RawEvent::ScheduleDispatchSuccess(1, 3));
		//assert!(System::events().iter().any(|record| record.event == schedule_dispatch_event));
	});
}
