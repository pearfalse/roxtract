use std::num::NonZeroU32;

use crate::{bintrinsics::Slice32, Offset};

#[cfg(feature = "crc")]
use {std::borrow::Borrow, crate::Rom};

/// Metadata about a known RISC OS ROM image.
#[non_exhaustive]
pub struct KnownRiscOsVersion {
	/// The OS version, in colloquial format (e.g. `RISC OS 3.11`).
	pub name_high_level: &'static str,
	/// The OS version string, as found in the ROM image.
	pub name_internal: &'static [u8],
	name_internal_pos: u32,
	/// The CRC32 hash of the ROM contents.
	pub crc32: u32,
}

macro_rules! known_version {
	($name:ident = $value:expr) => {
		#[cfg(feature = "crc")]
		static $name: KnownRiscOsVersion = $value;
	};
}

known_version!(ARTHUR_030 = KnownRiscOsVersion {
	name_high_level: "Arthur 0.30",
	name_internal: b"Arthur\t\t0.30 (17 Jun 1987)\0",
	name_internal_pos: 0x1460,
	crc32: 0x5df8ed42,
});

known_version!(ARTHUR_120 = KnownRiscOsVersion {
	name_high_level: "Arthur 1.20",
	name_internal: b"Arthur\t\t1.20 (25 Sep 1987)\0",
	name_internal_pos: 0x1318,
	crc32: 0xeb3fda57,
});

known_version!(RISC_OS_200 = KnownRiscOsVersion {
	name_high_level: "RISC OS 2.00",
	name_internal: b"RISC OS\t\t2.00 (05 Oct 1988)\0",
	name_internal_pos: 0x1b38,
	crc32: 0x89c4ad36,
});

known_version!(RISC_OS_201 = KnownRiscOsVersion {
	name_high_level: "RISC OS 2.01",
	name_internal: b"RISC OS\t\t2.01 (05 Jul 1990)\0",
	name_internal_pos: 0x4c90,
	crc32: 0x7cb5ea3f,
});

known_version!(RISC_OS_300 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.00",
	name_internal: b"RISC OS\t\t3.00 (25 Sep 1991)\0",
	name_internal_pos: 0x4854,
	crc32: 0xbfc99817,
});

known_version!(RISC_OS_310 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.10",
	name_internal: b"RISC OS\t\t3.10 (30 Apr 1992)\0",
	name_internal_pos: 0x498c,
	crc32: 0xecac4ea6,
});

known_version!(RISC_OS_311 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.11",
	name_internal: b"RISC OS\t\t3.11 (29 Sep 1992)\0",
	name_internal_pos: 0x498c,
	crc32: 0x54c0c963,
});

known_version!(RISC_OS_319 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.19",
	name_internal: b"RISC OS\t\t3.19 (9. Jun 1993)\0",
	name_internal_pos: 0x4a38,
	crc32: 0x00c7a3d3,
});

known_version!(RISC_OS_350 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.50",
	name_internal: b"RISC OS\t\t3.50 (18 Feb 1994)\0",
	name_internal_pos: 0x5134,
	crc32: 0x541b1415,
});

known_version!(RISC_OS_360 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.60",
	name_internal: b"RISC OS\t\t3.60 (13 Apr 1995)\0",
	name_internal_pos: 0x54b4,
	crc32: 0xa9822c2c,
});

known_version!(RISC_OS_370 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.70",
	name_internal: b"RISC OS\t\t3.70 (30 Jul 1996)\0",
	name_internal_pos: 0x55c4,
	crc32: 0x63fc131a,
});

known_version!(RISC_OS_371 = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.71",
	name_internal: b"RISC OS\t\t3.71 (19 Feb 1997)\0",
	name_internal_pos: 0x56e4,
	crc32: 0x211cf888,
});

#[cfg(feature = "crc")]
impl KnownRiscOsVersion {
	/// Returns `true` if the byte data in `rom` matches `self`.
	fn matches<M: Borrow<[u8]>>(&self, rom: &Rom<M>) -> bool {
		#[inline(never)]
		fn check_data(this: &KnownRiscOsVersion, rom_data: &[u8]) -> bool {
			let Some(slice_end) = this.name_internal_pos
				.checked_add(this.name_internal.len() as u32)
				.filter(|n| *n as usize <= rom_data.len())
			else { return false };

			if rom_data[this.name_internal_pos as usize .. slice_end as usize] != *this.name_internal {
				return false;
			}

			true
		}

		check_data(self, rom.borrow()) && rom.crc32_hash == self.crc32
	}

	/// Returns a reference to a `KnownRiscOsVersion` object, if there is one that matches
	/// the ROM image described in `rom_data`.
	pub fn find<M: Borrow<[u8]>>(rom: &Rom<M>) -> Option<&'static KnownRiscOsVersion> {
		macro_rules! consider {
			($($consider:ident),+ $(,)?) => {
				$(if $consider.matches(rom) {
					return Some(&$consider);
				})+

				return None;
			};
		}

		consider![
			RISC_OS_311, RISC_OS_371, RISC_OS_360, RISC_OS_201,
			RISC_OS_310, RISC_OS_300, RISC_OS_319,
			RISC_OS_370, RISC_OS_350,
			RISC_OS_200,
			ARTHUR_120, ARTHUR_030,
		];
	}
}

struct WordCursor<'a> {
	bytes: &'a Slice32,
	cursor_rel: u32,
}

impl<'a> WordCursor<'a> {
	pub fn new_start(bytes: &'a Slice32) -> Self {
		Self::new(bytes, |_| 0)
	}

	pub fn new_end(bytes: &'a Slice32) -> Self {
		Self::new(bytes, |b| b.len().saturating_sub(4))
	}

	fn new(bytes: &'a Slice32, make_start: impl FnOnce(&'a Slice32) -> u32) -> Self {
		let bytes_words_only = bytes.subslice(0..(bytes.len() & !3)).unwrap();

		Self {
			bytes: bytes_words_only,
			cursor_rel: make_start(bytes_words_only),
		}
	}

	pub fn current(&self) -> Option<u32> {
		// ensure we have four bytes in range
		if !matches!(self.cursor_rel.checked_add(4), Some(n) if n <= self.bytes.len()) {
			return None; // index if out of range
		}

		Some(unsafe {
			let ptr = self.bytes.as_ref().as_ptr().add(self.cursor_rel as usize).cast::<u32>();
			core::ptr::read_unaligned(ptr as *const u32)
		})
	}

	pub fn move_next(&mut self) {
		self.cursor_rel = self.cursor_rel.saturating_add(4); // saturation == guaranteed OOB
	}

	pub fn move_prev(&mut self) {
		self.cursor_rel = self.cursor_rel.wrapping_sub(4); // underflow == guaranteed OOB
	}

	pub fn pos(&self) -> u32 { self.cursor_rel }
}

impl Slice32 {
	/// Searches for `needle` in `self`, and returns a byte offset to it if found
	pub fn find_offset_to(&self, needle: &Slice32, offset: u32) -> Option<u32> {
		if self.len() < 4 { return None; }
		let target = Self::find(self, needle)?;
		let mut cursor = WordCursor::new_end(self.subslice(0..target)?);

		loop {
			let possible_start = cursor.pos().checked_sub(offset)?;
			if cursor.current().and_then(|cc| possible_start.checked_add(cc)) == Some(target) {
				return Some(possible_start);
			}
			cursor.move_prev();
		}
	}

	/// Finds the byte offset a word in `self` that functions as an offset to a copy of `needle`
	/// in `self`.
	///
	/// The `offset` parameter allows shifting the base of the relative addressing earlier by
	/// some number of bytes.
	pub fn find(&self, needle: &Slice32) -> Option<u32> {
		let mut haystack = self;
		if haystack.is_empty() { return None; }
		let (&needle_first, needle_rem) = needle.split_first()?;

		let mut hs_sub_start = 0u32;
		loop {
			let start = haystack.as_ref().iter().copied().position(move |n| n == needle_first)?
				as u32;

			let hs_range = (start + 1) .. (start + needle.len());
			if hs_range.end > haystack.len() {
				// remaining haystack is not long enough
				return None;
			}

			// first byte matches, compare remaining
			if haystack.subslice(hs_range.clone()) == Some(needle_rem) {
				// hs_range is relative to the subslice, not the original parameter
				return Some(hs_range.start as u32 - 1 + hs_sub_start);
			}

			haystack = haystack.subslice_from(hs_range.start).unwrap();
			hs_sub_start += hs_range.start;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn s(src: &[u8]) -> &Slice32 { Slice32::new(src).unwrap() }

	#[test]
	fn find() {
		assert_eq!(s(b"abcdef").find(s(b"abc")), Some(0));
		assert_eq!(s(b"abc").find(s(b"abc")), Some(0));
		assert_eq!(s(b"abcdef").find(s(b"bc")), Some(1));
		assert_eq!(s(b"aabc").find(s(b"abc")), Some(1));
		assert_eq!(s(b"ababc").find(s(b"abc")), Some(2));
		assert_eq!(s(b"abac").find(s(b"abc")), None);
		assert_eq!(s(b"cbabc").find(s(b"abc")), Some(2));
		assert_eq!(s(b"bac").find(s(b"a")), Some(1));

		assert_eq!(s(b"").find(s(b"empty haystack")), None);
		assert_eq!(s(b"empty needle").find(s(b"")), None);
	}

	#[test]
	fn find_offset_to() {
		assert_eq!(s(b"\x08\0\0\0ABCDEFGH").find_offset_to(s(b"EFGH"), 0), Some(0));
		assert_eq!(s(b"!!!!\x08\0\0\0ABCDEFGH").find_offset_to(s(b"EFGH"), 0), Some(4));
		assert_eq!(s(b"!!!!\x04\0\0\0EFGH").find_offset_to(s(b"EFGH"), 0), Some(4));
		assert_eq!(s(b"!!!!????ZERO\x08\0\0\0EFGH").find_offset_to(s(b"EFGH"), 4), Some(8));

		assert_eq!(s(&[
			b'o', b'f', b'f', b's', b'e', b't', b'!', b'!',
			0,0,0,0, // run         r00 a08
			0,0,0,0, // init        r04 a0c
			0,0,0,0, // fini        r08 a10
			0,0,0,0, // svc         r0c a14
			0x2c, 0,0,0, // title   r10 a18
			0,0,0,0, // help        r14 a1c
			0,0,0,0, // cmd         r18 a20
			0,0,0,0, // swi#        r1c a24
			0,0,0,0, // swi handler r20 a28
			0,0,0,0, // swi table   r24 a2c
			0,0,0,0, // swi code    r28 a30
			b'M', b'o', b'd', b'u', b'l', b'e', 0 // r2c a34
		]).find_offset_to(s(b"Module\0"), 0x10), Some(8));
	}

	#[test]
	fn find_offset_to_force_unaligned() {
		#![allow(unstable_name_collisions)]
		use sptr::Strict as _;

		static DATA: &[u8] = b"\x08\0\0\0!no!HELLO\0";
		let mut heap_data = vec![0u8; DATA.len() + 1].into_boxed_slice();
		let data = match (&heap_data[0] as *const u8).addr() & 3 {
			0 => &mut heap_data[1..],
			_ => &mut heap_data[..(DATA.len())]
		};
		data.copy_from_slice(DATA);
		assert_ne!(data.as_ptr().addr() & 3, 0);
		assert_eq!(s(data).find_offset_to(s(b"HELLO\0"), 0), Some(0));
	}
}

// Unit tests for these functions are up one level

pub(crate) fn kernel_start(data: &Slice32) -> Option<Offset> {
	let is_in_range = {
		let len = data.len();
		move |pos: &u32| *pos < len
	};

	let mut end_of_module0 = data.find(Slice32::new(b"MODULE#\0").unwrap())
		.and_then(|p| p.checked_add(8).filter(is_in_range))?;

	// Arthur 0.x up through RISC OS 2.00 have two extra non-zero words between `MODULE#0\0`
	// and the faux-module header; skip over them if present
	let mut word_skip = 2;
	while word_skip > 0 && data.read_word(end_of_module0)? != 0 {
		end_of_module0 = end_of_module0.checked_add(4).filter(is_in_range)?;
		word_skip -= 1;
	}

	NonZeroU32::new(end_of_module0)
}

pub(crate) fn kernel_version_str_pos(data: &Slice32, kernel_start: Offset) -> Option<Offset> {
	let kernel_start = kernel_start;
	let kernel_title_offset = kernel_start.checked_add(0x14) // title offset
		.and_then(|o| data.read_word(o.get()))
		.and_then(NonZeroU32::new)
		?;

	kernel_start.checked_add(kernel_title_offset.get())
}

pub(crate) fn kernel_version_str(data: &Slice32, start_pos: Option<Offset>)-> Option<&Slice32> {
	start_pos
		.and_then(|pos| data.subslice_from(pos.get()))
		.and_then(Slice32::cstr)
}

pub(crate) fn module_chain_start(data: &Slice32) -> Option<Offset> {
	const NEEDLE: &Slice32 = Slice32::new_unwrapped(b"UtilityModule\0");

	data.find_offset_to(NEEDLE, 0x10)
		.and_then(|n| n.checked_sub(4))
		.and_then(NonZeroU32::new)
}