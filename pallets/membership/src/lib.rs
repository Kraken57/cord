// This file is part of CORD – https://cord.network

// Copyright (C) Parity Technologies (UK) Ltd.
// Copyright (C) Dhiway Networks Pvt. Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later
// Adapted to meet the requirements of the CORD project.

// CORD is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// CORD is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with CORD. If not, see <https://www.gnu.org/licenses/>.
//
//! # Membership Module
//!
//! Allows control of membership of a set of `AccountId`s, useful for managing
//! membership of a collective. A prime member may be set
//! Modified to work with CORD governance modules.
// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
	traits::{ChangeMembers, Contains, Get, InitializeMembers, SortedMembers},
	BoundedVec,
};
use sp_runtime::traits::{IsMember, StaticLookup, UniqueSaturatedInto};
use sp_std::prelude::*;

pub mod weights;

pub use pallet::*;
pub use weights::WeightInfo;

type AccountIdLookupOf<T> = <<T as frame_system::Config>::Lookup as StaticLookup>::Source;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	/// The current storage version.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(4);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T, I = ()>(PhantomData<(T, I)>);

	#[pallet::config]
	pub trait Config<I: 'static = ()>: frame_system::Config {
		/// The overarching event type.
		type RuntimeEvent: From<Event<Self, I>>
			+ IsType<<Self as frame_system::Config>::RuntimeEvent>;

		type IsMember: IsMember<Self::AccountId>;
		/// Required origin for adding a member (though can always be Root).
		type AddOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Required origin for removing a member (though can always be Root).
		type RemoveOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Required origin for adding and removing a member in a single action.
		type SwapOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Required origin for resetting membership.
		type ResetOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Required origin for setting or resetting the prime member.
		type PrimeOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// The receiver of the signal for when the membership has been
		/// initialized. This happens pre-genesis and will usually be the same
		/// as `MembershipChanged`. If you need to do something different on
		/// initialization, then you can change this accordingly.
		type MembershipInitialized: InitializeMembers<Self::AccountId>;

		/// The receiver of the signal for when the membership has changed.
		type MembershipChanged: ChangeMembers<Self::AccountId>;

		/// The maximum number of members that this membership can have.
		///
		/// This is used for benchmarking. Re-run the benchmarks if this
		/// changes.
		///
		/// This is enforced in the code; the membership size can not exceed
		/// this limit.
		type MaxMembers: Get<u32>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	/// The current membership, stored as an ordered Vec.
	#[pallet::storage]
	#[pallet::getter(fn members)]
	pub type Members<T: Config<I>, I: 'static = ()> =
		StorageValue<_, BoundedVec<T::AccountId, T::MaxMembers>, ValueQuery>;

	/// The current prime member, if one exists.
	#[pallet::storage]
	#[pallet::getter(fn prime)]
	pub type Prime<T: Config<I>, I: 'static = ()> = StorageValue<_, T::AccountId, OptionQuery>;

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config<I>, I: 'static = ()> {
		pub members: BoundedVec<T::AccountId, T::MaxMembers>,
		#[serde(skip)]
		pub phantom: PhantomData<I>,
	}

	#[pallet::genesis_build]
	impl<T: Config<I>, I: 'static> BuildGenesisConfig for GenesisConfig<T, I> {
		fn build(&self) {
			use sp_std::collections::btree_set::BTreeSet;
			let members_set: BTreeSet<_> = self.members.iter().collect();
			assert_eq!(
				members_set.len(),
				self.members.len(),
				"Members cannot contain duplicate accounts."
			);

			let mut members = self.members.clone();
			members.sort();
			T::MembershipInitialized::initialize_members(&members);
			<Members<T, I>>::put(members);
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config<I>, I: 'static = ()> {
		/// The given member was added; see the transaction for who.
		MemberAdded,
		/// The given member was removed; see the transaction for who.
		MemberRemoved,
		/// Two members were swapped; see the transaction for who.
		MembersSwapped,
		/// The membership was reset; see the transaction for who the new set
		/// is.
		MembersReset,
		/// One of the members' keys changed.
		KeyChanged,
		/// Phantom member, never used.
		Dummy { _phantom_data: PhantomData<(T::AccountId, <T as Config<I>>::RuntimeEvent)> },
	}

	#[pallet::error]
	pub enum Error<T, I = ()> {
		/// Already a member.
		AlreadyMember,
		/// Not a member.
		NotMember,
		/// Too many members.
		TooManyMembers,
	}

	#[pallet::call]
	impl<T: Config<I>, I: 'static> Pallet<T, I> {
		/// Add a member `who` to the set.
		///
		/// May only be called from `T::AddOrigin`.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::add_member(T::MaxMembers::get()))]
		pub fn add_member(
			origin: OriginFor<T>,
			who: AccountIdLookupOf<T>,
		) -> DispatchResultWithPostInfo {
			T::AddOrigin::ensure_origin(origin)?;
			let who = T::Lookup::lookup(who)?;

			ensure!(T::IsMember::is_member(&who), Error::<T, I>::NotMember);

			let mut members = <Members<T, I>>::get();
			let init_length = members.len();
			let location = members.binary_search(&who).err().ok_or(Error::<T, I>::AlreadyMember)?;
			members
				.try_insert(location, who.clone())
				.map_err(|_| Error::<T, I>::TooManyMembers)?;

			<Members<T, I>>::put(&members);

			T::MembershipChanged::change_members_sorted(&[who], &[], &members[..]);

			Self::deposit_event(Event::MemberAdded);

			Ok(Some(T::WeightInfo::add_member(init_length as u32)).into())
		}

		/// Remove a member `who` from the set.
		///
		/// May only be called from `T::RemoveOrigin`.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::remove_member(T::MaxMembers::get()))]
		pub fn remove_member(
			origin: OriginFor<T>,
			who: AccountIdLookupOf<T>,
		) -> DispatchResultWithPostInfo {
			T::RemoveOrigin::ensure_origin(origin)?;
			let who = T::Lookup::lookup(who)?;

			let mut members = <Members<T, I>>::get();
			let init_length = members.len();
			let location = members.binary_search(&who).ok().ok_or(Error::<T, I>::NotMember)?;
			members.remove(location);

			<Members<T, I>>::put(&members);

			T::MembershipChanged::change_members_sorted(&[], &[who], &members[..]);
			Self::rejig_prime(&members);

			Self::deposit_event(Event::MemberRemoved);
			Ok(Some(T::WeightInfo::remove_member(init_length as u32)).into())
		}

		/// Swap out one member `remove` for another `add`.
		///
		/// May only be called from `T::SwapOrigin`.
		///
		/// Prime membership is *not* passed from `remove` to `add`, if extant.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::swap_member(T::MaxMembers::get()))]
		pub fn swap_member(
			origin: OriginFor<T>,
			remove: AccountIdLookupOf<T>,
			add: AccountIdLookupOf<T>,
		) -> DispatchResultWithPostInfo {
			T::SwapOrigin::ensure_origin(origin)?;
			let remove = T::Lookup::lookup(remove)?;
			let add = T::Lookup::lookup(add)?;

			if remove == add {
				return Ok(().into());
			}
			ensure!(T::IsMember::is_member(&add), Error::<T, I>::NotMember);

			let mut members = <Members<T, I>>::get();
			let location = members.binary_search(&remove).ok().ok_or(Error::<T, I>::NotMember)?;
			let _ = members.binary_search(&add).err().ok_or(Error::<T, I>::AlreadyMember)?;
			members[location] = add.clone();
			members.sort();

			<Members<T, I>>::put(&members);

			T::MembershipChanged::change_members_sorted(&[add], &[remove], &members[..]);
			Self::rejig_prime(&members);

			Self::deposit_event(Event::MembersSwapped);
			Ok(Some(T::WeightInfo::swap_member(members.len() as u32)).into())
		}

		/// Change the membership to a new set, disregarding the existing
		/// membership. Be nice and pass `members` pre-sorted.
		///
		/// May only be called from `T::ResetOrigin`.
		#[pallet::call_index(3)]
		#[pallet::weight(T::WeightInfo::reset_members(members.len().unique_saturated_into()))]
		pub fn reset_members(origin: OriginFor<T>, members: Vec<T::AccountId>) -> DispatchResult {
			T::ResetOrigin::ensure_origin(origin)?;

			let mut members: BoundedVec<T::AccountId, T::MaxMembers> =
				BoundedVec::try_from(members).map_err(|_| Error::<T, I>::TooManyMembers)?;
			members.sort();
			<Members<T, I>>::mutate(|m| {
				T::MembershipChanged::set_members_sorted(&members[..], m);
				Self::rejig_prime(&members);
				*m = members;
			});

			Self::deposit_event(Event::MembersReset);
			Ok(())
		}

		/// Swap out the sending member for some other key `new`.
		///
		/// May only be called from `Signed` origin of a current member.
		///
		/// Prime membership is passed from the origin account to `new`, if
		/// extant.
		#[pallet::call_index(4)]
		#[pallet::weight(T::WeightInfo::change_key(T::MaxMembers::get()))]
		pub fn change_key(
			origin: OriginFor<T>,
			new: AccountIdLookupOf<T>,
		) -> DispatchResultWithPostInfo {
			let remove = ensure_signed(origin)?;
			let new = T::Lookup::lookup(new)?;

			ensure!(T::IsMember::is_member(&new), Error::<T, I>::NotMember);
			if remove == new {
				return Ok(().into());
			}

			let mut members = <Members<T, I>>::get();
			let members_length = members.len() as u32;
			let location = members.binary_search(&remove).ok().ok_or(Error::<T, I>::NotMember)?;
			let _ = members.binary_search(&new).err().ok_or(Error::<T, I>::AlreadyMember)?;
			members[location] = new.clone();
			members.sort();

			<Members<T, I>>::put(&members);

			T::MembershipChanged::change_members_sorted(
				&[new.clone()],
				&[remove.clone()],
				&members[..],
			);

			if Prime::<T, I>::get() == Some(remove) {
				Prime::<T, I>::put(&new);
				T::MembershipChanged::set_prime(Some(new));
			}

			Self::deposit_event(Event::KeyChanged);
			Ok(Some(T::WeightInfo::change_key(members_length)).into())
		}

		/// Set the prime member. Must be a current member.
		///
		/// May only be called from `T::PrimeOrigin`.
		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::set_prime(T::MaxMembers::get()))]
		pub fn set_prime(
			origin: OriginFor<T>,
			who: AccountIdLookupOf<T>,
		) -> DispatchResultWithPostInfo {
			T::PrimeOrigin::ensure_origin(origin)?;
			let who = T::Lookup::lookup(who)?;
			let members = Self::members();
			members.binary_search(&who).ok().ok_or(Error::<T, I>::NotMember)?;
			Prime::<T, I>::put(&who);
			T::MembershipChanged::set_prime(Some(who));
			Ok(Some(T::WeightInfo::set_prime(members.len() as u32)).into())
		}

		/// Remove the prime member if it exists.
		///
		/// May only be called from `T::PrimeOrigin`.
		#[pallet::call_index(6)]
		#[pallet::weight(T::WeightInfo::clear_prime())]
		pub fn clear_prime(origin: OriginFor<T>) -> DispatchResult {
			T::PrimeOrigin::ensure_origin(origin)?;
			Prime::<T, I>::kill();
			T::MembershipChanged::set_prime(None);
			Ok(())
		}
	}
}

impl<T: Config<I>, I: 'static> Pallet<T, I> {
	fn rejig_prime(members: &[T::AccountId]) {
		if let Some(prime) = Prime::<T, I>::get() {
			match members.binary_search(&prime) {
				Ok(_) => T::MembershipChanged::set_prime(Some(prime)),
				Err(_) => Prime::<T, I>::kill(),
			}
		}
	}
}

impl<T: Config<I>, I: 'static> Contains<T::AccountId> for Pallet<T, I> {
	fn contains(t: &T::AccountId) -> bool {
		Self::members().binary_search(t).is_ok()
	}
}

impl<T: Config<I>, I: 'static> SortedMembers<T::AccountId> for Pallet<T, I> {
	fn sorted_members() -> Vec<T::AccountId> {
		Self::members().to_vec()
	}

	fn count() -> usize {
		Members::<T, I>::decode_len().unwrap_or(0)
	}
}

#[cfg(feature = "runtime-benchmarks")]
mod benchmark {
	use super::{Pallet as Membership, *};
	use frame_benchmarking::v1::{account, benchmarks_instance_pallet, whitelist, BenchmarkError};
	use frame_support::{assert_ok, traits::EnsureOrigin};
	use frame_system::RawOrigin;

	const SEED: u32 = 0;

	fn set_members<T: Config<I>, I: 'static>(members: Vec<T::AccountId>, prime: Option<usize>) {
		let reset_origin = T::ResetOrigin::try_successful_origin()
			.expect("ResetOrigin has no successful origin required for the benchmark");
		let prime_origin = T::PrimeOrigin::try_successful_origin()
			.expect("PrimeOrigin has no successful origin required for the benchmark");

		assert_ok!(<Membership<T, I>>::reset_members(reset_origin, members.clone()));
		if let Some(prime) = prime.map(|i| members[i].clone()) {
			let prime_lookup = T::Lookup::unlookup(prime);
			assert_ok!(<Membership<T, I>>::set_prime(prime_origin, prime_lookup));
		} else {
			assert_ok!(<Membership<T, I>>::clear_prime(prime_origin));
		}
	}

	benchmarks_instance_pallet! {
		add_member {
			let m in 1 .. (T::MaxMembers::get() - 1);

			let members = (0..m).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
			set_members::<T, I>(members, None);
			let new_member = account::<T::AccountId>("add", m, SEED);
			let new_member_lookup = T::Lookup::unlookup(new_member.clone());
		}: {
			assert_ok!(<Membership<T, I>>::add_member(
				T::AddOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
				new_member_lookup,
			));
		} verify {
			assert!(<Members<T, I>>::get().contains(&new_member));
			#[cfg(test)] crate::tests::clean();
		}

		// the case of no prime or the prime being removed is surely cheaper than the case of
		// reporting a new prime via `MembershipChanged`.
		remove_member {
			let m in 2 .. T::MaxMembers::get();

			let members = (0..m).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
			set_members::<T, I>(members.clone(), Some(members.len() - 1));

			let to_remove = members.first().cloned().unwrap();
			let to_remove_lookup = T::Lookup::unlookup(to_remove.clone());
		}: {
			assert_ok!(<Membership<T, I>>::remove_member(
				T::RemoveOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
				to_remove_lookup,
			));
		} verify {
			assert!(!<Members<T, I>>::get().contains(&to_remove));
			// prime is rejigged
			assert!(<Prime<T, I>>::get().is_some() && T::MembershipChanged::get_prime().is_some());
			#[cfg(test)] crate::tests::clean();
		}

		// we remove a non-prime to make sure it needs to be set again.
		swap_member {
			let m in 2 .. T::MaxMembers::get();

			let members = (0..m).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
			set_members::<T, I>(members.clone(), Some(members.len() - 1));
			let add = account::<T::AccountId>("member", m, SEED);
			let add_lookup = T::Lookup::unlookup(add.clone());
			let remove = members.first().cloned().unwrap();
			let remove_lookup = T::Lookup::unlookup(remove.clone());
		}: {
			assert_ok!(<Membership<T, I>>::swap_member(
				T::SwapOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
				remove_lookup,
				add_lookup,
			));
		} verify {
			assert!(!<Members<T, I>>::get().contains(&remove));
			assert!(<Members<T, I>>::get().contains(&add));
			// prime is rejigged
			assert!(<Prime<T, I>>::get().is_some() && T::MembershipChanged::get_prime().is_some());
			#[cfg(test)] crate::tests::clean();
		}

		// er keep the prime common between incoming and outgoing to make sure it is rejigged.
		reset_members {
			let m in 1 .. T::MaxMembers::get();

			let members = (1..m+1).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
			set_members::<T, I>(members.clone(), Some(members.len() - 1));
			let mut new_members = (m..2*m).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
		}: {
			assert_ok!(<Membership<T, I>>::reset_members(
				T::ResetOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
				new_members.clone(),
			));
		} verify {
			new_members.sort();
			assert_eq!(<Members<T, I>>::get(), new_members);
			// prime is rejigged
			assert!(<Prime<T, I>>::get().is_some() && T::MembershipChanged::get_prime().is_some());
			#[cfg(test)] crate::tests::clean();
		}

		change_key {
			let m in 1 .. T::MaxMembers::get();

			// worse case would be to change the prime
			let members = (0..m).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
			let prime = members.last().cloned().unwrap();
			set_members::<T, I>(members.clone(), Some(members.len() - 1));

			let add = account::<T::AccountId>("member", m, SEED);
			let add_lookup = T::Lookup::unlookup(add.clone());
			whitelist!(prime);
		}: {
			assert_ok!(<Membership<T, I>>::change_key(RawOrigin::Signed(prime.clone()).into(), add_lookup));
		} verify {
			assert!(!<Members<T, I>>::get().contains(&prime));
			assert!(<Members<T, I>>::get().contains(&add));
			// prime is rejigged
			assert_eq!(<Prime<T, I>>::get().unwrap(), add);
			#[cfg(test)] crate::tests::clean();
		}

		set_prime {
			let m in 1 .. T::MaxMembers::get();
			let members = (0..m).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
			let prime = members.last().cloned().unwrap();
			let prime_lookup = T::Lookup::unlookup(prime);
			set_members::<T, I>(members, None);
		}: {
			assert_ok!(<Membership<T, I>>::set_prime(
				T::PrimeOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
				prime_lookup,
			));
		} verify {
			assert!(<Prime<T, I>>::get().is_some());
			assert!(<T::MembershipChanged>::get_prime().is_some());
			#[cfg(test)] crate::tests::clean();
		}

		clear_prime {
			let members = (0..T::MaxMembers::get()).map(|i| account("member", i, SEED)).collect::<Vec<T::AccountId>>();
			let prime = members.last().cloned().unwrap();
			set_members::<T, I>(members, None);
		}: {
			assert_ok!(<Membership<T, I>>::clear_prime(
				T::PrimeOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?,
			));
		} verify {
			assert!(<Prime<T, I>>::get().is_none());
			assert!(<T::MembershipChanged>::get_prime().is_none());
			#[cfg(test)] crate::tests::clean();
		}

		impl_benchmark_test_suite!(Membership, crate::tests::new_bench_ext(), crate::tests::Test);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate as pallet_membership;

	use frame_support::{
		assert_noop, assert_ok, assert_storage_noop, derive_impl, ord_parameter_types,
		parameter_types, traits::ConstU32,
	};
	use frame_system::{pallet_prelude::BlockNumberFor, EnsureRoot, EnsureSignedBy};
	use sp_runtime::{bounded_vec, traits::BadOrigin, BuildStorage};

	type Block = frame_system::mocking::MockBlock<Test>;

	frame_support::construct_runtime!(
		pub enum Test
		{
			System: frame_system,
			Membership: pallet_membership,
			NetworkMembership: pallet_network_membership,
		}
	);

	parameter_types! {
		pub static Members: Vec<u64> = vec![];
		pub static Prime: Option<u64> = None;
	}

	#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
	impl frame_system::Config for Test {
		type Block = Block;
	}
	ord_parameter_types! {
		pub const One: u64 = 1;
		pub const Two: u64 = 2;
		pub const Three: u64 = 3;
		pub const Four: u64 = 4;
		pub const Five: u64 = 5;
	}

	pub struct TestChangeMembers;
	impl ChangeMembers<u64> for TestChangeMembers {
		fn change_members_sorted(incoming: &[u64], outgoing: &[u64], new: &[u64]) {
			let mut old_plus_incoming = Members::get();
			old_plus_incoming.extend_from_slice(incoming);
			old_plus_incoming.sort();
			let mut new_plus_outgoing = new.to_vec();
			new_plus_outgoing.extend_from_slice(outgoing);
			new_plus_outgoing.sort();
			assert_eq!(old_plus_incoming, new_plus_outgoing);

			Members::set(new.to_vec());
			Prime::set(None);
		}
		fn set_prime(who: Option<u64>) {
			Prime::set(who);
		}
		fn get_prime() -> Option<u64> {
			Prime::get()
		}
	}

	impl InitializeMembers<u64> for TestChangeMembers {
		fn initialize_members(members: &[u64]) {
			MEMBERS.with(|m| *m.borrow_mut() = members.to_vec());
		}
	}
	parameter_types! {
		pub const MembershipPeriod: BlockNumberFor<Test> = 5;
		pub const MaxMembersPerBlock: u32 = 5;
	}

	impl pallet_network_membership::Config for Test {
		type NetworkMembershipOrigin = EnsureRoot<u64>;
		type RuntimeEvent = RuntimeEvent;
		type MembershipPeriod = MembershipPeriod;
		type MaxMembersPerBlock = MaxMembersPerBlock;
		type WeightInfo = ();
	}

	pub struct TestIsNetworkMember;
	#[cfg(not(feature = "runtime-benchmarks"))]
	impl IsMember<u64> for TestIsNetworkMember {
		fn is_member(member_id: &u64) -> bool {
			(1..=30).contains(member_id)
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	impl IsMember<<Test as frame_system::Config>::AccountId> for TestIsNetworkMember {
		fn is_member(_account_id: &<Test as frame_system::Config>::AccountId) -> bool {
			// For benchmarking, assume all generated accounts are members
			true
		}
	}

	impl Config for Test {
		type RuntimeEvent = RuntimeEvent;
		type IsMember = TestIsNetworkMember;
		type AddOrigin = EnsureSignedBy<One, u64>;
		type RemoveOrigin = EnsureSignedBy<Two, u64>;
		type SwapOrigin = EnsureSignedBy<Three, u64>;
		type ResetOrigin = EnsureSignedBy<Four, u64>;
		type PrimeOrigin = EnsureSignedBy<Five, u64>;
		type MembershipInitialized = TestChangeMembers;
		type MembershipChanged = TestChangeMembers;
		type MaxMembers = ConstU32<10>;
		type WeightInfo = ();
	}

	pub(crate) fn new_test_ext() -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
		// We use default for brevity, but you can configure as desired if needed.
		pallet_membership::GenesisConfig::<Test> {
			members: bounded_vec![10, 20, 30],
			..Default::default()
		}
		.assimilate_storage(&mut t)
		.unwrap();
		t.into()
	}

	#[cfg(feature = "runtime-benchmarks")]
	pub(crate) fn new_bench_ext() -> sp_io::TestExternalities {
		frame_system::GenesisConfig::<Test>::default().build_storage().unwrap().into()
	}

	#[cfg(feature = "runtime-benchmarks")]
	pub(crate) fn clean() {
		Members::set(vec![]);
		Prime::set(None);
	}

	#[test]
	fn query_membership_works() {
		new_test_ext().execute_with(|| {
			assert_eq!(Membership::members(), vec![10, 20, 30]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), vec![10, 20, 30]);
		});
	}

	#[test]
	fn prime_member_works() {
		new_test_ext().execute_with(|| {
			assert_noop!(Membership::set_prime(RuntimeOrigin::signed(4), 20), BadOrigin);
			assert_noop!(
				Membership::set_prime(RuntimeOrigin::signed(5), 15),
				Error::<Test, _>::NotMember
			);
			assert_ok!(Membership::set_prime(RuntimeOrigin::signed(5), 20));
			assert_eq!(Membership::prime(), Some(20));
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());

			assert_ok!(Membership::clear_prime(RuntimeOrigin::signed(5)));
			assert_eq!(Membership::prime(), None);
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());
		});
	}

	#[test]
	fn add_member_works() {
		new_test_ext().execute_with(|| {
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 10, true));
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 15, true));
			assert_noop!(Membership::add_member(RuntimeOrigin::signed(5), 15), BadOrigin);
			assert_noop!(
				Membership::add_member(RuntimeOrigin::signed(1), 10),
				Error::<Test, _>::AlreadyMember
			);
			assert_ok!(Membership::add_member(RuntimeOrigin::signed(1), 15));
			assert_eq!(Membership::members(), vec![10, 15, 20, 30]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
		});
	}

	#[test]
	fn remove_member_works() {
		new_test_ext().execute_with(|| {
			assert_noop!(Membership::remove_member(RuntimeOrigin::signed(5), 20), BadOrigin);
			assert_noop!(
				Membership::remove_member(RuntimeOrigin::signed(2), 15),
				Error::<Test, _>::NotMember
			);
			assert_ok!(Membership::set_prime(RuntimeOrigin::signed(5), 20));
			assert_ok!(Membership::remove_member(RuntimeOrigin::signed(2), 20));
			assert_eq!(Membership::members(), vec![10, 30]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
			assert_eq!(Membership::prime(), None);
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());
		});
	}

	#[test]
	fn swap_member_works() {
		new_test_ext().execute_with(|| {
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 20, true));
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 25, true));
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 30, true));
			assert_noop!(Membership::swap_member(RuntimeOrigin::signed(5), 10, 25), BadOrigin);
			assert_noop!(
				Membership::swap_member(RuntimeOrigin::signed(3), 15, 25),
				Error::<Test, _>::NotMember
			);
			assert_noop!(
				Membership::swap_member(RuntimeOrigin::signed(3), 10, 30),
				Error::<Test, _>::AlreadyMember
			);

			assert_ok!(Membership::set_prime(RuntimeOrigin::signed(5), 20));
			assert_ok!(Membership::swap_member(RuntimeOrigin::signed(3), 20, 20));
			assert_eq!(Membership::members(), vec![10, 20, 30]);
			assert_eq!(Membership::prime(), Some(20));
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());

			assert_ok!(Membership::set_prime(RuntimeOrigin::signed(5), 10));
			assert_ok!(Membership::swap_member(RuntimeOrigin::signed(3), 10, 25));
			assert_eq!(Membership::members(), vec![20, 25, 30]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
			assert_eq!(Membership::prime(), None);
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());
		});
	}

	#[test]
	fn swap_member_works_that_does_not_change_order() {
		new_test_ext().execute_with(|| {
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 5, true));
			assert_ok!(Membership::swap_member(RuntimeOrigin::signed(3), 10, 5));
			assert_eq!(Membership::members(), vec![5, 20, 30]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
		});
	}

	#[test]
	fn swap_member_with_identical_arguments_changes_nothing() {
		new_test_ext().execute_with(|| {
			assert_storage_noop!(assert_ok!(Membership::swap_member(
				RuntimeOrigin::signed(3),
				10,
				10
			)));
		});
	}

	#[test]
	fn change_key_works() {
		new_test_ext().execute_with(|| {
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 20, true));
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 25, true));
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 40, true));
			assert_ok!(Membership::set_prime(RuntimeOrigin::signed(5), 10));
			assert_noop!(
				Membership::change_key(RuntimeOrigin::signed(3), 25),
				Error::<Test, _>::NotMember
			);
			assert_noop!(
				Membership::change_key(RuntimeOrigin::signed(10), 20),
				Error::<Test, _>::AlreadyMember
			);
			assert_ok!(Membership::change_key(RuntimeOrigin::signed(10), 40));
			assert_eq!(Membership::members(), vec![20, 30, 40]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
			assert_eq!(Membership::prime(), Some(40));
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());
		});
	}

	#[test]
	fn change_key_works_that_does_not_change_order() {
		new_test_ext().execute_with(|| {
			assert_ok!(NetworkMembership::nominate(RuntimeOrigin::root(), 5, true));
			assert_ok!(Membership::change_key(RuntimeOrigin::signed(10), 5));
			assert_eq!(Membership::members(), vec![5, 20, 30]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
		});
	}
	#[test]
	fn change_key_with_same_caller_as_argument_changes_nothing() {
		new_test_ext().execute_with(|| {
			assert_storage_noop!(assert_ok!(Membership::change_key(RuntimeOrigin::signed(10), 10)));
		});
	}

	#[test]
	fn reset_members_works() {
		new_test_ext().execute_with(|| {
			assert_ok!(Membership::set_prime(RuntimeOrigin::signed(5), 20));
			assert_noop!(
				Membership::reset_members(RuntimeOrigin::signed(1), bounded_vec![20, 40, 30]),
				BadOrigin
			);

			assert_ok!(Membership::reset_members(RuntimeOrigin::signed(4), vec![20, 40, 30]));
			assert_eq!(Membership::members(), vec![20, 30, 40]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
			assert_eq!(Membership::prime(), Some(20));
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());

			assert_ok!(Membership::reset_members(RuntimeOrigin::signed(4), vec![10, 40, 30]));
			assert_eq!(Membership::members(), vec![10, 30, 40]);
			assert_eq!(MEMBERS.with(|m| m.borrow().clone()), Membership::members().to_vec());
			assert_eq!(Membership::prime(), None);
			assert_eq!(PRIME.with(|m| *m.borrow()), Membership::prime());
		});
	}

	#[test]
	#[should_panic(expected = "Members cannot contain duplicate accounts.")]
	fn genesis_build_panics_with_duplicate_members() {
		pallet_membership::GenesisConfig::<Test> {
			members: bounded_vec![1, 2, 3, 1],
			phantom: Default::default(),
		}
		.build_storage()
		.unwrap();
	}
}
