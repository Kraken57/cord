// This file is part of CORD – https://cord.network

// Copyright (C) Dhiway Networks Pvt. Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

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

//! # cord-test pallet
//!
//! Provides functionality used in unit-tests of numerous modules across
//! CORD that require functioning runtime. Some calls are allowed to be
//! submitted as unsigned extrinsics, however most of them requires signing.
//! Refer to `pallet::Call` for further details.

use alloc::{vec, vec::Vec};
use frame_support::{pallet_prelude::*, storage};
use sp_core::sr25519::Public;
use sp_runtime::{
	traits::Hash,
	transaction_validity::{
		InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
	},
};

pub use self::pallet::*;

const LOG_TARGET: &str = "cord_test_pallet";

#[frame_support::pallet(dev_mode)]
pub mod pallet {
	use super::*;
	use crate::TransferData;
	use frame_system::pallet_prelude::*;
	use sp_core::storage::well_known_keys;
	use sp_runtime::{traits::BlakeTwo256, transaction_validity::TransactionPriority, Perbill};

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {}

	#[pallet::storage]
	#[pallet::getter(fn authorities)]
	pub type Authorities<T> = StorageValue<_, Vec<Public>, ValueQuery>;

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		pub authorities: Vec<Public>,
		#[serde(skip)]
		pub _config: core::marker::PhantomData<T>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			<Authorities<T>>::put(self.authorities.clone());
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Legacy call used in transaction pool benchmarks.
		#[pallet::call_index(0)]
		#[pallet::weight(100)]
		pub fn bench_call(_origin: OriginFor<T>, _transfer: TransferData) -> DispatchResult {
			Ok(())
		}

		/// Implicitly fill a block body with some data.
		#[pallet::call_index(1)]
		#[pallet::weight(100)]
		pub fn include_data(origin: OriginFor<T>, _data: Vec<u8>) -> DispatchResult {
			frame_system::ensure_signed(origin)?;
			Ok(())
		}

		/// Put/delete some data from storage. Intended to use as an unsigned
		/// extrinsic.
		#[pallet::call_index(2)]
		#[pallet::weight(100)]
		pub fn storage_change(
			_origin: OriginFor<T>,
			key: Vec<u8>,
			value: Option<Vec<u8>>,
		) -> DispatchResult {
			match value {
				Some(value) => storage::unhashed::put_raw(&key, &value),
				None => storage::unhashed::kill(&key),
			}
			Ok(())
		}

		/// Write a key value pair to the offchain database.
		#[pallet::call_index(3)]
		#[pallet::weight(100)]
		pub fn offchain_index_set(
			origin: OriginFor<T>,
			key: Vec<u8>,
			value: Vec<u8>,
		) -> DispatchResult {
			frame_system::ensure_signed(origin)?;
			sp_io::offchain_index::set(&key, &value);
			Ok(())
		}

		/// Remove a key and an associated value from the offchain database.
		#[pallet::call_index(4)]
		#[pallet::weight(100)]
		pub fn offchain_index_clear(origin: OriginFor<T>, key: Vec<u8>) -> DispatchResult {
			frame_system::ensure_signed(origin)?;
			sp_io::offchain_index::clear(&key);
			Ok(())
		}

		/// Create an index for this call.
		#[pallet::call_index(5)]
		#[pallet::weight(100)]
		pub fn indexed_call(origin: OriginFor<T>, data: Vec<u8>) -> DispatchResult {
			frame_system::ensure_signed(origin)?;
			let content_hash = sp_io::hashing::blake2_256(&data);
			let extrinsic_index: u32 =
				storage::unhashed::get(well_known_keys::EXTRINSIC_INDEX).unwrap();
			sp_io::transaction_index::index(extrinsic_index, data.len() as u32, content_hash);
			Ok(())
		}

		/// Deposit given digest items into the system storage. They will be
		/// included in a header during finalization.
		#[pallet::call_index(6)]
		#[pallet::weight(100)]
		pub fn deposit_log_digest_item(
			_origin: OriginFor<T>,
			log: sp_runtime::generic::DigestItem,
		) -> DispatchResult {
			<frame_system::Pallet<T>>::deposit_log(log);
			Ok(())
		}

		/// This call is validated as `ValidTransaction` with given priority.
		#[pallet::call_index(7)]
		#[pallet::weight(100)]
		pub fn call_with_priority(
			_origin: OriginFor<T>,
			_priority: TransactionPriority,
		) -> DispatchResult {
			Ok(())
		}

		/// This call is validated as non-propagable `ValidTransaction`.
		#[pallet::call_index(8)]
		#[pallet::weight(100)]
		pub fn call_do_not_propagate(_origin: OriginFor<T>) -> DispatchResult {
			Ok(())
		}

		/// Fill the block weight up to the given ratio.
		#[pallet::call_index(9)]
		#[pallet::weight(*_ratio * T::BlockWeights::get().max_block)]
		pub fn fill_block(origin: OriginFor<T>, _ratio: Perbill) -> DispatchResult {
			ensure_signed(origin)?;
			Ok(())
		}

		/// Read X times from the state some data.
		///
		/// Panics if it can not read `X` times.
		#[pallet::call_index(10)]
		#[pallet::weight(100)]
		pub fn read(_origin: OriginFor<T>, count: u32) -> DispatchResult {
			Self::execute_read(count, false)
		}

		/// Read X times from the state some data and then panic!
		///
		/// Returns `Ok` if it didn't read anything.
		#[pallet::call_index(11)]
		#[pallet::weight(100)]
		pub fn read_and_panic(_origin: OriginFor<T>, count: u32) -> DispatchResult {
			Self::execute_read(count, true)
		}
	}

	impl<T: Config> Pallet<T> {
		fn execute_read(read: u32, panic_at_end: bool) -> DispatchResult {
			let mut next_key = vec![];
			for _ in 0..(read as usize) {
				if let Some(next) = sp_io::storage::next_key(&next_key) {
					// Read the value
					sp_io::storage::get(&next);

					next_key = next;
				} else {
					if panic_at_end {
						return Ok(());
					} else {
						panic!("Could not read {read} times from the state");
					}
				}
			}

			if panic_at_end {
				panic!("BYE")
			} else {
				Ok(())
			}
		}
	}

	#[pallet::validate_unsigned]
	impl<T: Config> ValidateUnsigned for Pallet<T> {
		type Call = Call<T>;

		fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			log::trace!(target: LOG_TARGET, "validate_unsigned {call:?}");
			match call {
				// Some tests do not need to be complicated with signer and nonce, some need
				// reproducible block hash (call signature can't be there).
				// Offchain testing requires storage_change.
				Call::deposit_log_digest_item { .. } |
				Call::storage_change { .. } |
				Call::read { .. } |
				Call::read_and_panic { .. } => Ok(ValidTransaction {
					provides: vec![BlakeTwo256::hash_of(&call).encode()],
					..Default::default()
				}),
				_ => Err(TransactionValidityError::Invalid(InvalidTransaction::Call)),
			}
		}
	}
}

pub fn validate_runtime_call<T: pallet::Config>(call: &pallet::Call<T>) -> TransactionValidity {
	log::trace!(target: LOG_TARGET, "validate_runtime_call {call:?}");
	match call {
		Call::call_do_not_propagate {} =>
			Ok(ValidTransaction { propagate: false, ..Default::default() }),
		Call::call_with_priority { priority } =>
			Ok(ValidTransaction { priority: *priority, ..Default::default() }),
		_ => Ok(Default::default()),
	}
}
