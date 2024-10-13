use std::{
	borrow::Borrow,
	mem::transmute,
	ops::Range,
	slice::from_raw_parts,
};

/// A thin wrapper around a byte slice, providing fallible, copying, 32-bit access operations.
/// The underlying slice is no larger than `i32::MAX`.
#[repr(transparent)]
#[derive(Debug, PartialEq, Eq)]
pub struct Slice32([u8]);

impl Slice32 {
	/// Returns the length of the slice.
	pub const fn len(&self) -> u32 { self.0.len() as u32 }

	const SIZE_LIMIT: usize = i32::MAX as usize;

	/// Constructs a new `Slice32`, if its length is within range.
	pub const fn new(src: &[u8]) -> Option<&Slice32> {
		if src.len() > Self::SIZE_LIMIT { return None; }
		Some(unsafe {
			// SAFETY: we're casting to a transparent wrapper type
			transmute(src)
		})
	}

	/// Constructs a new `Slice32` variant via a `Box` allocation. If the array is too large, the
	/// original `Box` is returned.
	pub fn new_boxed(src: Box<[u8]>) -> Result<Box<Slice32>, Box<[u8]>> {
		if src.len() > Self::SIZE_LIMIT { return Err(src); }

		Ok(unsafe {
			// SAFETY: we're casting to a transparent wrapper type, via Box
			transmute(src)
		})
	}

	/// Constructs a new `Slice32` without verifying its length.
	///
	/// # Safety
	///
	/// - Slice length must be no larger than `i32::MAX`.
	pub const unsafe fn new_unchecked(src: &[u8]) -> &Self {
		unsafe {
			// SAFETY: this is a sound cast to a transparent wrapper type, but for the sake of
			// other methods in this type, the caller must upload the max size constraint
			transmute(src)
		}
	}

	/// Reads a byte at the given index.
	#[inline]
	pub fn read_byte(&self, idx: u32) -> Option<u8> {
		self.0.get(idx as usize).copied()
	}

	/// Reads a word at the given index.
	///
	/// This memory access does _not_ need to be aligned, physically or logically.
	pub fn read_word(&self, idx: u32) -> Option<u32> {
		if idx.saturating_add(4) > self.len() {
			return None;
		}

		Some(unsafe {
			// SAFETY: we know the slice is big enough, and we don't require u32 alignment
			self.0.as_ptr().add(idx as usize).cast::<u32>().read_unaligned()
		})
	}

	/// Subslices `self` by the given range.
	///
	/// Returns `None` if the requested slice is not in range.
	pub fn subslice(&self, range: Range<u32>) -> Option<&Self> {
		if range.start > range.end { return None; }

		if range.start as usize > self.0.len() || range.end as usize > self.0.len() {
			return None;
		}

		Some(unsafe {
			// SAFETY: we've checked that the given range is within `self`
			self.subslice_unchecked(range)
		})
	}

	/// Subslices `self` by removing `new_start` bytes from the front.
	#[inline]
	pub fn subslice_from(&self, new_start: u32) -> Option<&Self> {
		self.subslice(new_start..(self.len()))
	}

	/// Returns `true` if `self` is an empty slice.
	#[inline]
	pub const fn is_empty(&self) -> bool { self.0.is_empty() }

	/// Returns the first byte in the slice, if it isn't empty.
	#[inline]
	pub const fn first(&self) -> Option<u8> {
		match self.0.first() {
			Some(n) => Some(*n),
			None => None,
		}
	}

	/// Splits the slice at the first byte
	#[inline]
	pub fn split_first(&self) -> Option<(&u8, &Slice32)> {
		self.0.split_first().map(|(f, rem)| (f, unsafe {
			// SAFETY: `rem` is a 1-truncated version of `self` and meets length criterion
			Slice32::new_unchecked(rem)
		}))
	}

	/// Interprets the start of `self` as being the first byte of a C-string, returning the rest.
	///
	/// Returns `None` if no terminator was found.
	pub fn cstr(&self) -> Option<&Self> {
		let mut n = 0;
		while self.read_byte(n)? != 0 { n += 1; }
		Some(unsafe {
			// SAFETY: we've checked every byte in the slice, and also know the terminator is there
			// we also won't pass a bogus range
			self.subslice_unchecked(0..n)
		})
	}


	unsafe fn subslice_unchecked(&self, range: Range<u32>) -> &Self {
		unsafe {
			// SAFETY: caller must ensure that `range` is valid, and in range for `self`
			let len = range.end.checked_sub(range.start).unwrap_unchecked();

			transmute(from_raw_parts(
				self.0.as_ptr().add(range.start as usize),
				len as usize,
			))
		}
	}
}

impl Borrow<[u8]> for Slice32 {
	#[inline(always)]
	fn borrow(&self) -> &[u8] {
		&self.0
	}
}

impl<'a> Borrow<[u8]> for &'a Slice32 {
	#[inline(always)]
	fn borrow(&self) -> &[u8] {
		&self.0
	}
}

impl AsRef<[u8]> for Slice32 {
	fn as_ref(&self) -> &[u8] {
		&self.0
	}
}


#[cfg(test)]
mod uat {
	use super::*;

	#[test]
	fn i_want_to() {
		static DATA: &Slice32 = unsafe { Slice32::new_unchecked(&[
			b'H', b'e', b'a', b'd', b'e', b'r', 0, 0,
			0x19, 0, 0, 0,
			0xab, 0, 0xcd, 0,

			// unaligned
			0xff, 0xea, 0x1d, 0x0d, 0x60,
		]) };

		assert_eq!(Some(b'e'), DATA.read_byte(1));
		assert_eq!(Some(0x19), DATA.read_word(8));
		assert_eq!(None, DATA.read_word(1<<20));

		// first, a straightforward comparison
		assert_eq!(Some(&DATA.as_ref()[..6]), DATA.subslice(0..6).map(AsRef::as_ref));
		// use pointers to suppress value comparison, to more rigorously test the result
		assert_eq!(
			Some(&DATA.as_ref()[..6] as *const [u8]),
			DATA.subslice(0..6).map(|s| s as *const Slice32 as *const [u8]),
		);

		// cstring
		assert_eq!(Some(&DATA.as_ref()[..6]), DATA.cstr().map(AsRef::as_ref));
		assert_eq!(Some(0), DATA.subslice_from(6).and_then(Slice32::cstr).map(Slice32::len));
	}
}
