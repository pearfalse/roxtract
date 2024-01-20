use core::{
	borrow::Borrow,
	iter::ExactSizeIterator,
	mem::transmute,
	ops::Range,
	slice::from_raw_parts,
};

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq)]
pub struct Slice32([u8]);

impl Slice32 {
	pub fn len(&self) -> u32 { self.0.len() as u32 }

	const SIZE_LIMIT: usize = i32::MAX as usize;

	pub const fn new(src: &[u8]) -> Option<&Slice32> {
		if src.len() > Self::SIZE_LIMIT { return None; }
		Some(unsafe {
			// SAFETY: we're casting to a transparent wrapper type
			transmute(src)
		})
	}

	pub fn new_boxed(src: Box<[u8]>) -> Result<Box<Slice32>, Box<[u8]>> {
		if src.len() > Self::SIZE_LIMIT { return Err(src); }

		Ok(unsafe {
			// SAFETY: we're casting to a transparent wrapper type, via Box
			transmute(src)
		})
	}

	/// # Safety
	///
	/// - Slice length must fit in a `u32`.
	pub const unsafe fn new_unchecked(src: &[u8]) -> &Self {
		unsafe {
			// SAFETY: this is a sound cast to a transparent wrapper type, but for the sake of
			// other methods in this type, the caller must upload the max size constraint
			transmute(src)
		}
	}

	#[inline]
	pub fn read_byte(&self, idx: u32) -> Option<u8> {
		self.0.get(idx as usize).copied()
	}

	pub fn read_word(&self, idx: u32) -> Option<u32> {
		if idx.saturating_add(4) > self.len() {
			return None;
		}

		Some(unsafe {
			// SAFETY: we know the slice is big enough, and we don't require u32 alignment
			self.0.as_ptr().add(idx as usize).cast::<u32>().read_unaligned()
		})
	}

	pub fn subslice(&self, range: Range<u32>) -> Option<&Self> {
		assert!(range.end >= range.start);

		if range.start as usize > self.0.len() || range.end as usize > self.0.len() {
			return None;
		}

		Some(unsafe {
			// SAFETY: we've checked that the given range is within `self`
			transmute(from_raw_parts(
				self.0.as_ptr().add(range.start as usize),
				ExactSizeIterator::len(&range)
			))
		})
	}

	#[inline]
	pub fn subslice_from(&self, new_start: u32) -> Option<&Self> {
		self.subslice(new_start..(self.len()))
	}

	#[inline]
	pub const fn is_empty(&self) -> bool { self.0.is_empty() }

	#[inline]
	pub const fn first(&self) -> Option<u8> {
		match self.0.first() {
			Some(n) => Some(*n),
			None => None,
		}
	}

	#[inline]
	pub fn split_first(&self) -> Option<(&u8, &Slice32)> {
		self.0.split_first().map(|(f, rem)| (f, unsafe {
			// SAFETY: `rem` is a 1-truncated version of `self` and meets length criterion
			Slice32::new_unchecked(rem)
		}))
	}

	pub fn cstr(&self) -> Option<&Self> {
		let mut n = 0;
		while self.read_byte(n)? != 0 { n += 1; }
		Some(self.subslice(0..n).unwrap())
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
