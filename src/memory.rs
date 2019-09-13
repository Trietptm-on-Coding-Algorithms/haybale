//! Implementation of a `Memory` based on a Boolector array and 64-bit cells.
//! Handles fully general read and write operations: arbitrary addresses,
//! sizes, and alignments.

use boolector::{Array, BV};
use crate::backend::{BtorRef, SolverRef};
use log::debug;
use reduce::Reduce;
use std::convert::TryInto;

#[derive(Clone, Debug)]
pub struct Memory {
    btor: BtorRef,
    mem: Array,
    cell_bytes_as_bv: BV,
    log_bits_in_byte_as_bv: BV,
    log_bits_in_byte_as_wide_bv: BV,
}

impl Memory {
    pub const INDEX_BITS: u32 = 64;  // memory takes 64-bit indices
    pub const CELL_BITS: u32 = 64;  // memory "cells" are also 64-bit sized; we will mask if smaller operations are needed
    pub const BITS_IN_BYTE: u32 = 8;
    pub const LOG_BITS_IN_BYTE: u32 = 3;  // log base 2 of BITS_IN_BYTE
    pub const CELL_BYTES: u32 = Self::CELL_BITS / Self::BITS_IN_BYTE;  // how many bytes in a cell
    pub const LOG_CELL_BYTES: u32 = 3;  // log base 2 of CELL_BYTES. This many of the bottom index bits determine cell offset.
    pub const CELL_OFFSET_MASK: u32 = 0x7;  // Applying this mask to the address gives the cell offset

    /// A new `Memory`, whose contents at all addresses are completely uninitialized (unconstrained)
    pub fn new_uninitialized(btor: BtorRef) -> Self {
        let log_num_cells = Self::INDEX_BITS - Self::LOG_CELL_BYTES;  // 2 to this number gives the number of memory cells
        Self {
            mem: Array::new(btor.clone().into(), log_num_cells, Self::CELL_BITS, Some("mem")),
            cell_bytes_as_bv: BV::from_u64(btor.clone().into(), u64::from(Self::CELL_BYTES), Self::INDEX_BITS),
            log_bits_in_byte_as_bv: BV::from_u64(btor.clone().into(), u64::from(Self::LOG_BITS_IN_BYTE), Self::CELL_BITS),
            log_bits_in_byte_as_wide_bv: BV::from_u64(btor.clone().into(), u64::from(Self::LOG_BITS_IN_BYTE), 2*Self::CELL_BITS),
            btor,  // out of order so it can be used above but moved in here
        }
    }

    /// A new `Memory`, whose contents at all addresses are initialized to be `0`
    pub fn new_zero_initialized(btor: BtorRef) -> Self {
        let log_num_cells = Self::INDEX_BITS - Self::LOG_CELL_BYTES;  // 2 to this number gives the number of memory cells
        Self {
            mem: Array::new_initialized(btor.clone().into(), log_num_cells, Self::CELL_BITS, Some("mem_initialized"), &BV::zero(btor.clone().into(), Self::CELL_BITS)),
            cell_bytes_as_bv: BV::from_u64(btor.clone().into(), u64::from(Self::CELL_BYTES), Self::INDEX_BITS),
            log_bits_in_byte_as_bv: BV::from_u64(btor.clone().into(), u64::from(Self::LOG_BITS_IN_BYTE), Self::CELL_BITS),
            log_bits_in_byte_as_wide_bv: BV::from_u64(btor.clone().into(), u64::from(Self::LOG_BITS_IN_BYTE), 2*Self::CELL_BITS),
            btor,  // out of order so it can be used above but moved in here
        }
    }

    /// Adapt the `Memory` to a new `Btor` instance.
    ///
    /// The new `Btor` instance should have been created (possibly transitively)
    /// via `Btor::duplicate()` from the `BtorRef` this `Memory` was originally
    /// created with (or most recently changed to). Further, no new variables
    /// should have been added since the call to `Btor::duplicate()`.
    pub fn change_solver(&mut self, new_btor: BtorRef) {
        self.mem = new_btor.match_array(&self.mem).unwrap();
        self.cell_bytes_as_bv = new_btor.match_bv(&self.cell_bytes_as_bv).unwrap();
        self.log_bits_in_byte_as_bv = new_btor.match_bv(&self.log_bits_in_byte_as_bv).unwrap();
        self.log_bits_in_byte_as_wide_bv = new_btor.match_bv(&self.log_bits_in_byte_as_wide_bv).unwrap();
        self.btor = new_btor;
    }

    /// Read an entire cell from the given address.
    /// If address is not cell-aligned, this will give the entire cell _containing_ that address.
    fn read_cell(&self, addr: &BV) -> BV {
        assert_eq!(addr.get_width(), Self::INDEX_BITS);
        let cell_num = addr.slice(Self::INDEX_BITS-1, Self::LOG_CELL_BYTES);  // discard the cell offset
        self.mem.read(&cell_num)
    }

    /// Write an entire cell to the given address.
    /// If address is not cell-aligned, this will write to the cell _containing_ that address, which is probably not what you want.
    // TODO: to enforce concretization, we could just take a u64 address here
    fn write_cell(&mut self, addr: &BV, val: BV) {
        assert_eq!(addr.get_width(), Self::INDEX_BITS);
        assert_eq!(val.get_width(), Self::CELL_BITS);
        let cell_num = addr.slice(Self::INDEX_BITS-1, Self::LOG_CELL_BYTES);  // discard the cell offset
        self.mem = self.mem.write(&cell_num, &val);
    }

    /// Read any number of bits of memory, at any alignment, but not crossing cell boundaries.
    /// Returned `BV` will have size `bits`.
    fn read_within_cell(&self, addr: &BV, bits: u32) -> BV {
        debug!("Reading within cell, {} bits at {:?}", bits, addr);
        let cell_contents = self.read_cell(addr);
        assert!(bits <= Self::CELL_BITS);
        let rval = if bits == Self::CELL_BITS {
            cell_contents  // shortcut to avoid more BV operations
                            // This assumes that `addr` was cell-aligned, but that must be the case if we're reading CELL_BITS bits and not crossing cell boundaries
        } else {
            let offset = addr.slice(Self::LOG_CELL_BYTES-1, 0)  // the actual offset part of the address
                .uext(Self::CELL_BITS - Self::LOG_CELL_BYTES)  // zero-extend to CELL_BITS
                .sll(&self.log_bits_in_byte_as_bv);  // offset in bits rather than bytes

            // We can't `slice` at a non-const location, but we can shift by a non-const amount
            cell_contents.srl(&offset)  // shift off whatever low-end bits we don't want
                .slice(bits - 1, 0)  // take just the bits we want, starting from 0
        };
        debug!("Value read is {:?}", rval);
        rval
    }

    /// Write any number of bits of memory, at any alignment, but not crossing cell boundaries.
    // TODO: to enforce concretization, we could just take a `u64` address here
    fn write_within_cell(&mut self, addr: &BV, val: BV) {
        debug!("Writing within cell, {:?} to address {:?}", val, addr);
        let write_size = val.get_width();
        assert!(write_size <= Self::CELL_BITS);
        let data_to_write = if write_size == Self::CELL_BITS {
            val  // shortcut to avoid more BV operations
                // This assumes that `addr` was cell-aligned, but that must be the case if we're writing CELL_BITS bits and not crossing cell boundaries
        } else {
            let offset = addr.slice(Self::LOG_CELL_BYTES-1, 0)  // the actual offset part of the address
                .uext(Self::CELL_BITS - Self::LOG_CELL_BYTES)  // zero-extend to CELL_BITS
                .sll(&self.log_bits_in_byte_as_bv);  // offset in bits rather than bytes

            // mask_clear is 0's in the bit positions that will be written, 1's elsewhere.
            // We construct the inverse of this mask, then bitwise negate it.
            let mask_clear = BV::ones(self.btor.clone().into(), write_size)  // a bitvector of ones, of width equal to the width that will be written
                .uext(Self::CELL_BITS - write_size)  // zero-extend to CELL_BITS
                .sll(&offset)  // now we have ones in the bit positions that will be written, zeroes elsewhere
                .not();  // the final desired mask

            // mask_write is the write data in its appropriate bit positions, 0's elsewhere.
            let mask_write = val.uext(Self::CELL_BITS - write_size).sll(&offset);

            self.read_cell(addr)
                .and(&mask_clear)  // zero out the section we'll be writing
                .or(&mask_write)  // write the data
        };
        debug!("Final cell data being written is {:?}", data_to_write);
        self.write_cell(addr, data_to_write);
    }

    /// Read up to a cell size's worth of memory, at any alignment. May cross cell boundaries.
    /// Returned `BV` will have size `bits`.
    fn read_small(&self, addr: &BV, bits: u32) -> BV {
        assert!(bits <= Self::CELL_BITS);
        if bits <= 8 {
            // In this case we can't possibly cross cell boundaries
            self.read_within_cell(addr, bits)
        } else {
            // We'll read this cell and the next cell, which between them must have all the data we need
            let next_cell_addr = addr.add(&self.cell_bytes_as_bv);
            let merged_contents = self.read_cell(&next_cell_addr).concat(&self.read_cell(addr));
            let offset = addr.slice(Self::LOG_CELL_BYTES-1, 0)  // the actual offset part of the address
                .uext(2*Self::CELL_BITS - Self::LOG_CELL_BYTES)  // zero-extend to 2*CELL_BITS
                .sll(&self.log_bits_in_byte_as_wide_bv);  // offset in bits rather than bytes

            // We can't `slice` at a non-const location, but we can shift by a non-const amount
            merged_contents.srl(&offset)  // shift off whatever low-end bits we don't want
                .slice(bits - 1, 0)  // take just the bits we want, starting from 0
        }
    }

    /// Write up to a cell size's worth of memory, at any alignment. May cross cell boundaries.
    fn write_small(&mut self, addr: &BV, val: BV) {
        let write_size = val.get_width();
        assert!(write_size <= Self::CELL_BITS);
        if write_size <= 8 {
            // In this case we can't possibly cross cell boundaries
            self.write_within_cell(addr, val);
        } else {
            // We'll allow for the possibility that the write crosses into the next cell
            let next_cell_addr = addr.add(&self.cell_bytes_as_bv);
            let offset = addr.slice(Self::LOG_CELL_BYTES-1, 0)  // the actual offset part of the address
                .uext(2*Self::CELL_BITS - Self::LOG_CELL_BYTES)  // zero-extend to 2*CELL_BITS
                .sll(&self.log_bits_in_byte_as_wide_bv);  // offset in bits rather than bytes

            // mask_clear is 0's in the bit positions that will be written, 1's elsewhere.
            // We construct the inverse of this mask, then bitwise negate it.
            let mask_clear = BV::ones(self.btor.clone().into(), write_size)  // a bitvector of ones, of width equal to the width that will be written
                .uext(2*Self::CELL_BITS - write_size)  // zero-extend to 2*CELL_BITS
                .sll(&offset)  // now we have ones in the bit positions that will be written, zeroes elsewhere
                .not();  // the final desired mask

            // mask_write is the write data in its appropriate bit positions, 0's elsewhere.
            let mask_write = val.uext(2*Self::CELL_BITS - write_size).sll(&offset);

            let data_to_write = self.read_cell(&next_cell_addr).concat(&self.read_cell(addr))  // existing data in the two cells
                .and(&mask_clear)  // zero out the section we'll be writing
                .or(&mask_write);  // write the data

            self.write_cell(addr, data_to_write.slice(Self::CELL_BITS-1, 0));  // first cell gets the low bits
            self.write_cell(&next_cell_addr, data_to_write.slice(2*Self::CELL_BITS-1, Self::CELL_BITS));  // second cell gets the high bits
        }
    }

    /// Read any number (>0) of bits of memory, but `addr` must be cell-aligned.
    /// Returned `BV` will have size `bits`.
    fn read_large_aligned(&self, addr: &BV, bits: u32) -> BV {
        assert_ne!(bits, 0);  // this function still technically works for small reads (just less efficient), so we only check for size 0 (which would break it)
        let num_full_cells = (bits-1) / Self::CELL_BITS;  // this is bits / CELL_BITS, but if bits is a multiple of CELL_BITS, it undercounts by 1 (we treat this as N-1 full cells plus a "partial" cell of CELL_BITS bits)
        let bits_in_last_cell = (bits-1) % Self::CELL_BITS + 1;  // this is bits % CELL_BITS, but if bits is a multiple of CELL_BITS, then we get CELL_BITS rather than 0
        itertools::repeat_n(Self::CELL_BITS, num_full_cells.try_into().unwrap())
            .chain(std::iter::once(bits_in_last_cell))  // this forms the sequence of read sizes
            .enumerate()
            .map(|(i,sz)| {
                let offset_bytes = i as u64 * u64::from(Self::CELL_BYTES);
                // note that all reads in the sequence must be within-cell, i.e., not cross cell boundaries, because of how we constructed the sequence
                self.read_within_cell(&addr.add(&BV::from_u64(self.btor.clone().into(), offset_bytes, Self::INDEX_BITS)), sz)
            })
            .reduce(|a,b| b.concat(&a))
            .unwrap()  // because of the std::iter::once, there must have been at least 1 item in the iterator
    }

    /// Write any number (>0) of bits of memory, but `addr` must be cell-aligned.
    fn write_large_aligned(&mut self, addr: &BV, val: BV) {
        let write_size = val.get_width();
        assert_ne!(write_size, 0);  // this function still technically works for small writes (just less efficient), so we only check for size 0 (which would break it)
        let num_full_cells = (write_size-1) / Self::CELL_BITS;  // this is bits / CELL_BITS, but if bits is a multiple of CELL_BITS, it undercounts by 1 (we treat this as N-1 full cells plus a "partial" cell of CELL_BITS bits)
        let bits_in_last_cell = (write_size-1) % Self::CELL_BITS + 1;  // this is bits % CELL_BITS, but if bits is a multiple of CELL_BITS, then we get CELL_BITS rather than 0
        let write_size_sequence = itertools::repeat_n(Self::CELL_BITS, num_full_cells.try_into().unwrap())
            .chain(std::iter::once(bits_in_last_cell));  // note that all writes in this sequence must be within-cell, i.e., not cross cell boundaries, because of how we constructed the sequence
        for (i,sz) in write_size_sequence.enumerate() {
            assert!(sz > 0);
            let offset_bytes = i as u64 * u64::from(Self::CELL_BYTES);
            let offset_bits = i as u32 * Self::CELL_BITS;
            let write_data = val.slice(sz + offset_bits - 1, offset_bits);
            self.write_within_cell(&addr.add(&BV::from_u64(self.btor.clone().into(), offset_bytes, Self::INDEX_BITS)), write_data);
        }
    }

    /// Read any number (>0) of bits of memory, at any alignment.
    /// Returned `BV` will have size `bits`.
    pub fn read(&self, addr: &BV, bits: u32) -> BV {
        debug!("Reading {} bits at {:?}", bits, addr);
        if bits <= Self::CELL_BITS {
            // special-case small reads because read_small() can handle them directly and efficiently
            self.read_small(addr, bits)
        } else {
            // Let's see if we can refactor this into a small read plus a large cell-aligned read
            if let Some(addr_u64) = addr.as_u64() {
                // addr is constrained to a single concrete value, which we could find without a solve. Yay!
                let cell_offset = addr_u64 & u64::from(Self::CELL_OFFSET_MASK);
                if cell_offset == 0 {
                    // the address is cell-aligned, and we're free to do the large read
                    self.read_large_aligned(addr, bits)
                } else {
                    let bytes_till_cell_boundary = u64::from(Self::CELL_BYTES) - cell_offset;
                    // first read the remainder of the cell to bring us to a cell boundary; this read must be <= Self::CELL_BITS
                    let first = self.read_small(addr, bytes_till_cell_boundary as u32 * Self::BITS_IN_BYTE);
                    // now read the rest, which will be a cell-aligned read
                    let next_cell_addr = addr.add(&BV::from_u64(self.btor.clone().into(), bytes_till_cell_boundary, addr.get_width()));
                    let rest = self.read_large_aligned(&next_cell_addr, bits - bytes_till_cell_boundary as u32 * Self::BITS_IN_BYTE);
                    // put them together and return
                    rest.concat(&first)
                }
            } else {
                // Not sure what the alignment of `addr` is, we'll just use the safe fallback
                let addr_size = addr.get_width();
                assert_eq!(bits % Self::BITS_IN_BYTE, 0);
                let bytes = bits / Self::BITS_IN_BYTE;
                assert!(bytes > 0);
                (0 .. bytes)
                    .map(|byte_num| {
                        let offset_addr = addr.add(&BV::from_u64(self.btor.clone().into(), u64::from(byte_num), addr_size));
                        self.read_within_cell(&offset_addr, Self::BITS_IN_BYTE)
                    })
                    .reduce(|a,b| b.concat(&a))
                    .unwrap()  // because bytes > 0, there must have been at least 1 item in the iterator
            }
        }
    }

    /// Write any number (>0) of bits of memory, at any alignment.
    pub fn write(&mut self, addr: &BV, val: BV) {
        debug!("Writing {:?} to address {:?}", val, addr);
        let write_size = val.get_width();
        if write_size <= Self::CELL_BITS {
            // special-case small writes because write_small() can handle them directly and efficiently
            self.write_small(addr, val)
        } else {
            // Let's see if we can refactor this into a small write plus a large cell-aligned write
            if let Some(addr_u64) = addr.as_u64() {
                // addr is constrained to a single concrete value, which we could find without a solve. Yay!
                let cell_offset = addr_u64 & u64::from(Self::CELL_OFFSET_MASK);
                if cell_offset == 0 {
                    // the address is cell-aligned, and we're free to do the large write
                    self.write_large_aligned(addr, val)
                } else {
                    let bytes_till_cell_boundary = u64::from(Self::CELL_BYTES) - cell_offset;
                    // first write the remainder of the cell to bring us to a cell boundary; this write must be <= Self::CELL_BITS
                    let first = val.slice(bytes_till_cell_boundary as u32 * Self::BITS_IN_BYTE - 1, 0);  // recall that the write is > Self::CELL_BITS, so this slice() must be valid
                    self.write_small(addr, first);
                    // now write the rest, which will be a cell-aligned write
                    let rest = val.slice(val.get_width() - 1, bytes_till_cell_boundary as u32 * Self::BITS_IN_BYTE);
                    let next_cell_addr = addr.add(&BV::from_u64(self.btor.clone().into(), bytes_till_cell_boundary, addr.get_width()));
                    self.write_large_aligned(&next_cell_addr, rest);
                }
            } else {
                // Not sure what the alignment of `addr` is, we'll just use the safe fallback
                let addr_size = addr.get_width();
                let val_size = val.get_width();
                assert_eq!(val_size % Self::BITS_IN_BYTE, 0);
                let val_bytes = val_size / Self::BITS_IN_BYTE;
                for byte_num in 0 .. val_bytes {
                    let val_byte = val.slice((byte_num+1) * Self::BITS_IN_BYTE - 1, byte_num * Self::BITS_IN_BYTE);
                    let offset_addr = addr.add(&BV::from_u64(self.btor.clone().into(), u64::from(byte_num), addr_size));
                    self.write_within_cell(&offset_addr, val_byte);
                }
            }
        }
    }
}

impl PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.btor == other.btor && self.mem == other.mem // we don't care about checking equality on `cell_bytes_as_bv`, `log_bits_in_byte_as_bv`, or `log_bits_in_byte_as_wide_bv`
    }
}

impl Eq for Memory {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::possible_solutions::*;
    use crate::sat::sat;
    use std::collections::HashSet;
    use std::iter::FromIterator;

    #[test]
    fn uninitialized() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mem = Memory::new_uninitialized(btor.clone());

        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        let zero = BV::zero(btor.clone().into(), Memory::CELL_BITS);

        // Read a value from (uninitialized) memory
        let read_bv = mem.read(&addr, Memory::CELL_BITS);

        // Constrain it to be > 0 and check that we're sat (and get a value > 0)
        btor.push(1);
        read_bv.sgt(&zero).assert();
        assert_eq!(sat(&btor), Ok(true));
        let read_val = read_bv.get_a_solution().as_u64().unwrap();
        assert!(read_val > 0);

        // Alternately, constrain it to be < 0 and check that we're sat (and get a value < 0)
        btor.pop(1);
        read_bv.slt(&zero).assert();
        assert_eq!(sat(&btor), Ok(true));
        let read_val = read_bv.get_a_solution().as_u64().unwrap() as i64;
        assert!(read_val < 0);
    }

    #[test]
    fn zero_initialized() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mem = Memory::new_zero_initialized(btor.clone());

        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);

        // Read a value from (zero-initialized) memory and check that the only possible value is 0
        let read_bv = mem.read(&addr, Memory::CELL_BITS);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0))));
    }

    #[test]
    fn read_and_write_to_cell_zero() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store a cell's worth of data to address 0
        let data_val = 0x1234_5678;
        let data = BV::from_u64(btor.clone().into(), data_val, Memory::CELL_BITS);
        let zero = BV::zero(btor.clone().into(), Memory::INDEX_BITS);
        mem.write(&zero, data);

        // Ensure that we can read it back again
        let read_bv = mem.read(&zero, Memory::CELL_BITS);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
    }

    #[test]
    fn read_and_write_cell_aligned() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store a cell's worth of data to a nonzero, but aligned, address
        let data_val = 0x1234_5678;
        let data = BV::from_u64(btor.clone().into(), data_val, Memory::CELL_BITS);
        let aligned = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&aligned, data);

        // Ensure that we can read it back again
        let read_bv = mem.read(&aligned, Memory::CELL_BITS);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
    }

    #[test]
    fn read_and_write_small() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 8 bits of data to an aligned address
        let data_val = 0x4F;
        let data = BV::from_u64(btor.clone().into(), data_val, 8);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, data);

        // Ensure that we can read it back again
        let read_bv = mem.read(&addr, 8);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
    }

    #[test]
    fn read_and_write_unaligned() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 8 bits of data to offset 1 in a cell
        let data_val = 0x4F;
        let data = BV::from_u64(btor.clone().into(), data_val, 8);
        let unaligned = BV::from_u64(btor.clone().into(), 0x10001, Memory::INDEX_BITS);
        mem.write(&unaligned, data);

        // Ensure that we can read it back again
        let read_bv = mem.read(&unaligned, 8);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
    }

    #[test]
    fn read_and_write_across_cell_boundaries() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 64 bits of data such that half is in one cell and half in the next
        let data_val: u64 = 0x12345678_9abcdef0;
        let data = BV::from_u64(btor.clone().into(), data_val, Memory::CELL_BITS);
        let addr = BV::from_u64(btor.clone().into(), 0x10004, Memory::INDEX_BITS);
        mem.write(&addr, data);

        // Ensure that we can read it back again
        let read_bv = mem.read(&addr, Memory::CELL_BITS);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
    }

    #[test]
    fn read_and_write_symbolic_addr() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 64 bits of data to a symbolic address
        let data_val: u64 = 0x12345678_9abcdef0;
        let data = BV::from_u64(btor.clone().into(), data_val, Memory::CELL_BITS);
        let addr = BV::new(btor.clone().into(), Memory::INDEX_BITS, Some("symbolic_addr"));
        mem.write(&addr, data);

        // Ensure that we can read it back again
        let read_bv = mem.read(&addr, Memory::CELL_BITS);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
    }

    #[test]
    fn read_and_write_twocells() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store two cells' worth of data to an aligned address
        let data_val_0: u64 = 0x12345678_9abcdef0;
        let data_val_1: u64 = 0x2468ace0_13579bdf;
        let write_val = BV::from_u64(btor.clone().into(), data_val_1, 64).concat(&BV::from_u64(btor.clone().into(), data_val_0, 64));
        assert_eq!(write_val.get_width(), 2*Memory::CELL_BITS);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, write_val);

        // Ensure that we can read it back again
        let read_bv = mem.read(&addr, 128);
        assert_eq!(sat(&btor), Ok(true));
        let read_val_0 = read_bv.slice(63, 0).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_0, data_val_0, "\nGot value 0x{:x}, expected 0x{:x}", read_val_0, data_val_0);
        let read_val_1 = read_bv.slice(127, 64).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_1, data_val_1);
    }

    #[test]
    fn read_and_write_200bits() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 200 bits of data to an aligned address
        let data_val_0: u64 = 0x12345678_9abcdef0;
        let data_val_1: u64 = 0x2468ace0_13579bdf;
        let data_val_2: u64 = 0xfedcba98_76543210;
        let data_val_3: u64 = 0xef;
        let write_val = BV::from_u64(btor.clone().into(), data_val_3, 8)
            .concat(&BV::from_u64(btor.clone().into(), data_val_2, 64))
            .concat(&BV::from_u64(btor.clone().into(), data_val_1, 64))
            .concat(&BV::from_u64(btor.clone().into(), data_val_0, 64));
        assert_eq!(write_val.get_width(), 200);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, write_val);

        // Ensure that we can read it back again
        let read_bv = mem.read(&addr, 200);
        assert_eq!(sat(&btor), Ok(true));
        let read_val_0 = read_bv.slice(63, 0).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_0, data_val_0);
        let read_val_1 = read_bv.slice(127, 64).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_1, data_val_1);
        let read_val_2 = read_bv.slice(191, 128).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_2, data_val_2);
        let read_val_3 = read_bv.slice(199, 192).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_3, data_val_3);
    }

    #[test]
    fn read_and_write_200bits_unaligned() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 200 bits of data to an unaligned address
        let data_val_0: u64 = 0x12345678_9abcdef0;
        let data_val_1: u64 = 0x2468ace0_13579bdf;
        let data_val_2: u64 = 0xfedcba98_76543210;
        let data_val_3: u64 = 0xef;
        let write_val = BV::from_u64(btor.clone().into(), data_val_3, 8)
            .concat(&BV::from_u64(btor.clone().into(), data_val_2, 64))
            .concat(&BV::from_u64(btor.clone().into(), data_val_1, 64))
            .concat(&BV::from_u64(btor.clone().into(), data_val_0, 64));
        assert_eq!(write_val.get_width(), 200);
        let addr = BV::from_u64(btor.clone().into(), 0x10003, Memory::INDEX_BITS);
        mem.write(&addr, write_val);

        // Ensure that we can read it back again
        let read_bv = mem.read(&addr, 200);
        assert_eq!(sat(&btor), Ok(true));
        let read_val_0 = read_bv.slice(63, 0).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_0, data_val_0);
        let read_val_1 = read_bv.slice(127, 64).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_1, data_val_1);
        let read_val_2 = read_bv.slice(191, 128).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_2, data_val_2);
        let read_val_3 = read_bv.slice(199, 192).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_3, data_val_3);
    }

    #[test]
    fn read_and_write_200bits_symbolic_addr() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 200 bits of data to a symbolic address
        let data_val_0: u64 = 0x12345678_9abcdef0;
        let data_val_1: u64 = 0x2468ace0_13579bdf;
        let data_val_2: u64 = 0xfedcba98_76543210;
        let data_val_3: u64 = 0xef;
        let write_val = BV::from_u64(btor.clone().into(), data_val_3, 8)
            .concat(&BV::from_u64(btor.clone().into(), data_val_2, 64))
            .concat(&BV::from_u64(btor.clone().into(), data_val_1, 64))
            .concat(&BV::from_u64(btor.clone().into(), data_val_0, 64));
        assert_eq!(write_val.get_width(), 200);
        let addr = BV::new(btor.clone().into(), Memory::INDEX_BITS, Some("symbolic_addr"));
        mem.write(&addr, write_val);

        // Ensure that we can read it back again
        let read_bv = mem.read(&addr, 200);
        assert_eq!(sat(&btor), Ok(true));
        let read_val_0 = read_bv.slice(63, 0).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_0, data_val_0);
        let read_val_1 = read_bv.slice(127, 64).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_1, data_val_1);
        let read_val_2 = read_bv.slice(191, 128).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_2, data_val_2);
        let read_val_3 = read_bv.slice(199, 192).get_a_solution().as_u64().unwrap();
        assert_eq!(read_val_3, data_val_3);
    }

    #[test]
    fn write_twice_read_once() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 8 bits of data
        let data_val = 0x4F;
        let data = BV::from_u64(btor.clone().into(), data_val, 8);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, data);

        // Store a different 8 bits of data to the same address
        let data_val = 0x3A;
        let data = BV::from_u64(btor.clone().into(), data_val, 8);
        mem.write(&addr, data);

        // Ensure that we get back the most recent data
        let read_bv = mem.read(&addr, 8);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone().into(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
    }

    #[test]
    fn write_different_cells() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 32 bits of data to a cell
        let data_val = 0x1234_5678;
        let data = BV::from_u64(btor.clone().into(), data_val, 32);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, data);

        // Store a different 32 bits of data to a different cell
        let data_val_2 = 0xfedc_ba98;
        let data_2 = BV::from_u64(btor.clone().into(), data_val_2, 32);
        let addr_2 = BV::from_u64(btor.clone().into(), 0x10008, Memory::INDEX_BITS);
        mem.write(&addr_2, data_2);

        // Ensure that we can read them both individually
        let read_bv = mem.read(&addr, 32);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
        let read_bv = mem.read(&addr_2, 32);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val_2))));
    }

    #[test]
    fn write_different_places_within_cell() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 32 bits of data to a cell
        let data_val = 0x1234_5678;
        let data = BV::from_u64(btor.clone().into(), data_val, 32);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, data);

        // Store a different 32 bits of data to the other half of the cell
        let data_val_2 = 0xfedc_ba98;
        let data_2 = BV::from_u64(btor.clone().into(), data_val_2, 32);
        let addr_2 = BV::from_u64(btor.clone().into(), 0x10004, Memory::INDEX_BITS);
        mem.write(&addr_2, data_2);

        // Ensure that we can read them both individually
        let read_bv = mem.read(&addr, 32);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val))));
        let read_bv = mem.read(&addr_2, 32);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(data_val_2))));
    }

    #[test]
    fn write_small_read_big() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_zero_initialized(btor.clone());

        // Store 8 bits of data to offset 1 in a cell
        let data_val = 0x4F;
        let data = BV::from_u64(btor.clone().into(), data_val, 8);
        let unaligned = BV::from_u64(btor.clone().into(), 0x10001, Memory::INDEX_BITS);
        mem.write(&unaligned, data.clone());

        // Ensure that reading from beginning of the cell adds zeroed low-order bits
        // (we are little-endian)
        let aligned = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        let read_bv = mem.read(&aligned, 16);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x4F00))));

        // Ensure that reading extra bits adds zeroed high-order bits
        let read_bv = mem.read(&unaligned, 16);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x004F))));

        // Ensure that reading elsewhere gives all zeroes
        let garbage_addr_1 = BV::from_u64(btor.clone().into(), 0x10004, Memory::INDEX_BITS);
        let garbage_addr_2 = BV::from_u64(btor.clone().into(), 0x10008, Memory::INDEX_BITS);
        let read_bv_1 = mem.read(&garbage_addr_1, 8);
        let read_bv_2 = mem.read(&garbage_addr_2, 8);
        assert_eq!(sat(&btor), Ok(true));
        let ps_1 = get_possible_solutions_for_bv(btor.clone(), &read_bv_1, 1).unwrap().as_u64_solutions().unwrap();
        let ps_2 = get_possible_solutions_for_bv(btor.clone(), &read_bv_2, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps_1, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0))));
        assert_eq!(ps_2, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0))));
    }

    #[test]
    fn write_big_read_small() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Store 32 bits of data to offset 2 in a cell
        let data_val = 0x1234_5678;
        let data = BV::from_u64(btor.clone().into(), data_val, 32);
        let offset_2 = BV::from_u64(btor.clone().into(), 0x10002, Memory::INDEX_BITS);
        mem.write(&offset_2, data.clone());

        // Ensure that reading 8 bits from offset 2 gives the low-order byte
        // (we are little-endian)
        let read_bv = mem.read(&offset_2, 8);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x78))));

        // Ensure that reading 8 bits from offset 5 gives the high-order byte
        // (we are little-endian)
        let offset_5 = BV::from_u64(btor.clone().into(), 0x10005, Memory::INDEX_BITS);
        let read_bv = mem.read(&offset_5, 8);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x12))));

        // Ensure that reading 16 bits from offset 3 gives the middle two bytes
        let offset_3 = BV::from_u64(btor.clone().into(), 0x10003, Memory::INDEX_BITS);
        let read_bv = mem.read(&offset_3, 16);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x3456))));
    }

    #[test]
    fn partial_overwrite_aligned() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Write an entire cell
        let data = BV::from_u64(btor.clone().into(), 0x12345678_12345678, Memory::CELL_BITS);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, data);

        // Write over just the first part
        let overwrite_data_val = 0xdcba;
        let overwrite_data = BV::from_u64(btor.clone().into(), overwrite_data_val, 16);
        mem.write(&addr, overwrite_data);

        // Ensure that we can read the smaller overwrite back
        let read_bv = mem.read(&addr, 16);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(overwrite_data_val))));

        // Ensure that reading the whole cell back reflects the partial overwrite
        let read_bv = mem.read(&addr, Memory::CELL_BITS);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x12345678_1234dcba))));
    }

    #[test]
    fn partial_overwrite_unaligned() {
        let _ = env_logger::builder().is_test(true).try_init();
        let btor = BtorRef::default();
        let mut mem = Memory::new_uninitialized(btor.clone());

        // Write an entire cell
        let data = BV::from_u64(btor.clone().into(), 0x12345678_12345678, Memory::CELL_BITS);
        let addr = BV::from_u64(btor.clone().into(), 0x10000, Memory::INDEX_BITS);
        mem.write(&addr, data);

        // Write over just part of the middle
        let overwrite_addr = BV::from_u64(btor.clone().into(), 0x10002, Memory::INDEX_BITS);
        let overwrite_data_val = 0xdcba;
        let overwrite_data = BV::from_u64(btor.clone().into(), overwrite_data_val, 16);
        mem.write(&overwrite_addr, overwrite_data);

        // Ensure that we can read the smaller overwrite back
        let read_bv = mem.read(&overwrite_addr, 16);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(overwrite_data_val))));

        // Ensure that reading the whole cell back reflects the partial overwrite
        let read_bv = mem.read(&addr, Memory::CELL_BITS);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x12345678_dcba5678))));

        // Now a different partial read with some original data and some overwritten
        let new_addr = BV::from_u64(btor.clone().into(), 0x10003, Memory::INDEX_BITS);
        let read_bv = mem.read(&new_addr, 16);
        assert_eq!(sat(&btor), Ok(true));
        let ps = get_possible_solutions_for_bv(btor.clone(), &read_bv, 1).unwrap().as_u64_solutions().unwrap();
        assert_eq!(ps, PossibleSolutions::PossibleSolutions(HashSet::from_iter(std::iter::once(0x78dc))));
    }
}
