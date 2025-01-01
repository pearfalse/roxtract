use core::fmt;
use std::num::{NonZeroU16, NonZeroU8};

use crate::Slice32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
	pub version: Version,
	pub date: ReleaseDate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Version {
	data: NonZeroU16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseDate {
	day: NonZeroU8,
	month: ReleaseMonth,
	year: NonZeroU16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMonth {
	January = 1,
	February = 2,
	March = 3,
	April = 4,
	May = 5,
	June = 6,
	July = 7,
	August = 8,
	September = 9,
	October = 10,
	November = 11,
	December = 12,
}

// rudimentary decimal digit parse
#[inline]
fn parse_digit(ch: u8) -> Option<u8> {
	match ch {
		b'0'..=b'9' => Some(ch - b'0'),
		_ => None,
	}
}

fn parse_digits(digits: &Slice32) -> Option<u16> {
	let mut result = 0;
	for &digit in digits.as_ref().iter() {
		result = result * 10 + parse_digit(digit)? as u16;
	}
	Some(result)
}

impl Release {
	pub fn parse(mut src: &Slice32) -> Option<Self> {
		// NAME\t\tV.VV (DD Mmm YYYY)

		const TWO_TABS: &Slice32 = Slice32::new(b"\t\t").unwrap();
		if let Some(first_tab_pos) = src.find(TWO_TABS)
		{
			src = src.subslice_from(first_tab_pos + 2).unwrap()
		} else { return None }

		// trim string to version
		let (version, date_part) =
		if let [a, b'.', b, c, b' ', b'(', ref rest @ .., b')'] = *src.as_ref() {
			let (a, b, c) = (parse_digit(a)?, parse_digit(b)?, parse_digit(c)?);
			(
				Version { data: NonZeroU16::new({
					let a = (a as u16) << 8;
					let b = (b as u16) * 10;
					let c = c as u16;
					a + b + c
				})? },
				Slice32::new(rest).unwrap()
			)
		} else { return None };

		// parse date
		let date = ReleaseDate::parse(date_part)?;
		Some(Release { version, date })
	}
}

impl Version {
	#[inline]
	pub const fn major(self) -> u8 { (self.data.get() >> 8) as u8 }

	#[inline]
	pub const fn minor(self) -> u8 { self.data.get() as u8 }
}

impl fmt::Debug for Version {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Version))
			.field("major", &self.major())
			.field("minor", &self.minor())
			.finish()
	}
}

impl ReleaseDate {
	pub fn parse(src: &Slice32) -> Option<Self> {
		let year = src.subslice_last(4)
			.and_then(|s| parse_digits(s))
			.and_then(NonZeroU16::new)?;

		// RISC OS 3.19 has the date without a leading 0 digit :(
		let month_part = if let [_, b' ', _,_,_, b' ', ..] = *src.as_ref() {
			src.subslice(2..5) // should never fail
		} else {
			src.subslice(3..6) // could fail. we've checked nothing here
		}?;

		let month = ReleaseMonth::parse(month_part)?;

		let day  = match *src.as_ref() {
			[d, b' ', ..]
				=> parse_digit(d),
			[_d1, _d2, b' ', ..]
				=> parse_digits(src.subslice(0..2).unwrap())
					.map(|i| i as u8), // two decimal digits always fits
			_ => None
		}.and_then(NonZeroU8::new)?;

		Some(ReleaseDate { day, month, year })
	}

	#[inline]
	pub const fn day(self) -> NonZeroU8 { self.day }

	#[inline]
	pub const fn month(self) -> ReleaseMonth { self.month }

	#[inline]
	pub const fn year(self) -> NonZeroU16 { self.year }
}

impl ReleaseMonth {
	pub fn parse(src: &Slice32) -> Option<Self> {
		match <[u8; 3]>::try_from(src.as_ref()).ok()? {
			[b'J',b'a',b'n'] => Some(Self::January),
			[b'F',b'e',b'b'] => Some(Self::February),
			[b'M',b'a',b'r'] => Some(Self::March),
			[b'A',b'p',b'r'] => Some(Self::April),
			[b'M',b'a',b'y'] => Some(Self::May),
			[b'J',b'u',b'n'] => Some(Self::June),
			[b'J',b'u',b'l'] => Some(Self::July),
			[b'A',b'u',b'g'] => Some(Self::August),
			[b'S',b'e',b'p'] => Some(Self::September),
			[b'O',b'c',b't'] => Some(Self::October),
			[b'N',b'o',b'v'] => Some(Self::November),
			[b'D',b'e',b'c'] => Some(Self::December),
			_ => None,
		}
	}
}

impl crate::Recell for Release {
	const FIND_FAILURE: Self = Release {
		version: Version { data: NonZeroU16::MAX },
		date: ReleaseDate {
			day: NonZeroU8::MAX, month: ReleaseMonth::December, year: NonZeroU16::MAX
		}
	};
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn release_dates() {
		assert_eq!(
			Some(ReleaseDate {
				day: NonZeroU8::new(31).unwrap(),
				month: ReleaseMonth::January,
				year: NonZeroU16::new(1992).unwrap(),
			}),
			ReleaseDate::parse(Slice32::new(b"31 Jan 1992").unwrap())
		);

		assert_eq!(
			Some(ReleaseDate {
				day: NonZeroU8::new(6).unwrap(),
				month: ReleaseMonth::April,
				year: NonZeroU16::new(1472).unwrap(),
			}),
			ReleaseDate::parse(Slice32::new(b"6 Apr 1472").unwrap())
		);
	}
}

