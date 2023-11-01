use crate::{Rom, bintrinsics::Slice32};

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

impl Rom {
	pub fn find_offset_to(haystack: &Slice32, needle: &Slice32, offset: u32) -> Option<u32> {
		if haystack.len() < 4 { return None; }
		let target = Self::find(haystack, needle)?;
		let mut cursor = WordCursor::new_end(haystack.subslice(0..target)?);

		loop {
			let possible_start = cursor.pos().checked_sub(offset)?;
			if cursor.current().and_then(|cc| possible_start.checked_add(cc)) == Some(target) {
				return Some(possible_start);
			}
			cursor.move_prev();
		}
	}

	pub fn find(mut haystack: &Slice32, needle: &Slice32) -> Option<u32> {
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
		assert_eq!(Rom::find(s(b"abcdef"), s(b"abc")), Some(0));
		assert_eq!(Rom::find(s(b"abc"), s(b"abc")), Some(0));
		assert_eq!(Rom::find(s(b"abcdef"), s(b"bc")), Some(1));
		assert_eq!(Rom::find(s(b"aabc"), s(b"abc")), Some(1));
		assert_eq!(Rom::find(s(b"ababc"), s(b"abc")), Some(2));
		assert_eq!(Rom::find(s(b"abac"), s(b"abc")), None);
		assert_eq!(Rom::find(s(b"cbabc"), s(b"abc")), Some(2));
		assert_eq!(Rom::find(s(b"bac"), s(b"a")), Some(1));

		assert_eq!(Rom::find(s(b""), s(b"empty haystack")), None);
		assert_eq!(Rom::find(s(b"empty needle"), s(b"")), None);
	}

	#[test]
	fn find_offset_to() {
		assert_eq!(Rom::find_offset_to(s(b"\x08\0\0\0ABCDEFGH"), s(b"EFGH"), 0), Some(0));
		assert_eq!(Rom::find_offset_to(s(b"!!!!\x08\0\0\0ABCDEFGH"), s(b"EFGH"), 0), Some(4));
		assert_eq!(Rom::find_offset_to(s(b"!!!!\x04\0\0\0EFGH"), s(b"EFGH"), 0), Some(4));
		assert_eq!(Rom::find_offset_to(s(b"!!!!????ZERO\x08\0\0\0EFGH"), s(b"EFGH"), 4), Some(8));

		assert_eq!(Rom::find_offset_to(s(&[
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
		]), s(b"Module\0"), 0x10), Some(8));
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
		assert_eq!(Rom::find_offset_to(s(data), s(b"HELLO\0"), 0), Some(0));
	}
}