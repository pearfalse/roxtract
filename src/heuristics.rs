use std::{debug_assert, borrow::Borrow, ops::Range};

use crate::Rom;

#[non_exhaustive]
pub struct KnownRiscOsVersion {
	name_high_level: &'static str,
	name_internal: &'static [u8],
	name_internal_pos: u32,
	crc32: u32,
}

static RISC_OS_311: KnownRiscOsVersion = KnownRiscOsVersion {
	name_high_level: "RISC OS 3.11",
	name_internal: b"RISC OS\t\t3.11 (29 Sep 1992)\0",
	name_internal_pos: 0x498c,
	crc32: 0x54c0c963,
};

impl KnownRiscOsVersion {
	pub fn matches(&self, rom_data: &[u8]) -> bool {
		let Some(slice_end) = self.name_internal_pos.checked_add(self.name_internal.len() as u32)
			.filter(|n| *n as usize <= rom_data.len())
		else { return false };

		if rom_data[self.name_internal_pos as usize .. slice_end as usize] != *self.name_internal {
			return false;
		}

		let mut hasher = crc_any::CRCu32::crc32();
		hasher.digest(rom_data);
		hasher.get_crc() == self.crc32
	}
}


pub trait SliceExt {
	fn get_word(&self, pos: usize) -> Option<u32>;
	fn get_all(&self, range: Range<u32>) -> Option<&[u8]>;
}

impl SliceExt for [u8] {
	fn get_word(&self, pos: usize) -> Option<u32> {
		let pos_top = pos.checked_add(4).filter(|n| *n <= self.len())?;
		Some(u32::from_le_bytes([
			self[pos_top - 4], self[pos_top - 3], self[pos_top - 2], self[pos_top - 1]
		]))
	}

	fn get_all(&self, range: Range<u32>) -> Option<&[u8]> {
		let in_range = {
			let limit = self.len();
			move |n: &u32| (*n as usize) <= limit
		};

		Some(range.start).filter(in_range)?;
		Some(range.end).filter(in_range)?;

		Some(&self[(range.start as usize)..(range.end as usize)])
	}
}

#[cfg(test)]
mod test_slice_ext {
	use super::SliceExt;
	use assert_hex::assert_eq_hex;

	#[test]
	fn get_word() {
		const SRC: &[u8] = b"murderous".as_slice();
		assert_eq_hex!(Some(0x6472756d), SRC.get_word(0));
		assert_eq_hex!(Some(0x65647275), SRC.get_word(1));
		assert_eq_hex!(Some(0x73756f72), SRC.get_word(5));
		assert!(SRC.get_word(6).is_none());
		assert!(SRC.get_word(9).is_none());
		assert!(SRC.get_word(9999).is_none());
	}

	#[test]
	fn get_all() {
		const SRC: &[u8] = b"stone building".as_slice();
		assert_eq!(Some(&b"stone"[..]), SRC.get_all(0..5));
		assert_eq!(Some(&b"building"[..]), SRC.get_all(6..14));
		assert_eq!(None, SRC.get_all(6..15));
		assert_eq!(None, SRC.get_all(20..21));
	}
}


struct WordCursor<'a> {
	bytes: &'a [u8],
	cursor_rel: u32,
}

impl<'a> WordCursor<'a> {
	pub fn new_start<T: Borrow<[u8]> + ?Sized>(bytes: &T) -> Self {
		Self::new(bytes.borrow(),|_| 0)
	}

	pub fn new_end<T: Borrow<[u8]> + ?Sized>(bytes: &T) -> Self {
		Self::new(bytes.borrow(),|b| (b.len() as u32).saturating_sub(4))
	}

	fn new(bytes: &[u8], make_start: impl FnOnce(&[u8]) -> u32) -> Self {
		let bytes: &[u8] = bytes.borrow();
		assert!(bytes.len() <= i32::MAX as usize);

		let bytes_words_only = unsafe {
			// SAFETY: this can only reduce the size of `bytes`
			core::slice::from_raw_parts(bytes.as_ptr(), bytes.len() & !3)
		};

		Self {
			bytes: bytes_words_only,
			cursor_rel: make_start(bytes_words_only),
		}
	}

	pub fn current(&self) -> Option<u32> {
		// ensure we have four bytes in range
		if !matches!(self.cursor_rel.checked_add(4), Some(n) if n as usize <= self.bytes.len()) {
			return None; // index if out of range
		}

		Some(unsafe {
			let ptr = self.bytes.as_ptr().add(self.cursor_rel as usize).cast::<u32>();
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

impl Rom {

	pub fn find_offset_to(haystack: &[u8], needle: &[u8], offset: u32) -> Option<u32> {
		if haystack.len() < 4 { return None; }
		let target = Self::find(haystack, needle)?;
		let mut cursor = WordCursor::new_end(&haystack[..(target as usize)]);

		loop {
			let possible_start = cursor.pos().checked_sub(offset)?;
			if cursor.current().and_then(|cc| possible_start.checked_add(cc)) == Some(target) {
				return Some(possible_start);
			}
			cursor.move_prev();
		}
	}

	pub fn find(mut haystack: &[u8], needle: &[u8]) -> Option<u32> {
		debug_assert!(haystack.len() <= u32::MAX as usize);
		debug_assert!(needle.len() <= u32::MAX as usize);

		if haystack.is_empty() { return None; }
		let (&needle_first, needle_rem) = needle.split_first()?;

		let mut hs_sub_start = 0u32;
		loop {
			let start = haystack.iter().copied().position(move |n| n == needle_first)?;

			let hs_range = (start + 1) .. (start + needle.len());
			if hs_range.end > haystack.len() {
				// remaining haystack is not long enough
				return None;
			}

			// first byte matches, compare remaining
			if &haystack[hs_range.clone()] == needle_rem {
				// hs_range is relative to the subslice, not the original parameter
				return Some(hs_range.start as u32 - 1 + hs_sub_start);
			}

			haystack = &haystack[hs_range.start ..];
			hs_sub_start += hs_range.start as u32;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn find() {
		assert_eq!(Rom::find(b"abcdef", b"abc"), Some(0));
		assert_eq!(Rom::find(b"abc", b"abc"), Some(0));
		assert_eq!(Rom::find(b"abcdef", b"bc"), Some(1));
		assert_eq!(Rom::find(b"aabc", b"abc"), Some(1));
		assert_eq!(Rom::find(b"ababc", b"abc"), Some(2));
		assert_eq!(Rom::find(b"abac", b"abc"), None);
		assert_eq!(Rom::find(b"cbabc", b"abc"), Some(2));
		assert_eq!(Rom::find(b"bac", b"a"), Some(1));

		assert_eq!(Rom::find(b"", b"empty haystack"), None);
		assert_eq!(Rom::find(b"empty needle", b""), None);
	}

	#[test]
	fn find_offset_to() {
		assert_eq!(Rom::find_offset_to(b"\x08\0\0\0ABCDEFGH", b"EFGH", 0), Some(0));
		assert_eq!(Rom::find_offset_to(b"!!!!\x08\0\0\0ABCDEFGH", b"EFGH", 0), Some(4));
		assert_eq!(Rom::find_offset_to(b"!!!!\x04\0\0\0EFGH", b"EFGH", 0), Some(4));
		assert_eq!(Rom::find_offset_to(b"!!!!????ZERO\x08\0\0\0EFGH", b"EFGH", 4), Some(8));

		assert_eq!(Rom::find_offset_to(&[
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
		], b"Module\0", 0x10), Some(8));
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
		assert_eq!(Rom::find_offset_to(data, b"HELLO\0", 0), Some(0));
	}
}