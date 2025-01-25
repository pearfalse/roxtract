use core::fmt;
use std::num::{NonZeroU16, NonZeroU8};

use crate::Slice32;

/// Parsed release info for a ROM image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
	#[allow(missing_docs)] pub version: Version,
	#[allow(missing_docs)] pub date: ReleaseDate,
}

/// Parsed version information for a ROM image, derived from its `UtilityModule` help string.
///
/// This type supports any version that can be expressed as two `u8`s, where at least one of them
/// is not zero, and the major version `u8` is no greater than 9.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
	data: NonZeroU16,
}

/// Parsed release date information for a ROM image.
///
/// Dates must be expressed in the form `DD MMM YYYY`, where
///
/// - `DD` is a two-digit date between 1 and 31, or a single-digit date followed by `.`;
/// - `MMM` is the first three letters of the month name, with one leading capital letter (e.g.
///   `Jan`, `Apr`);
/// - `YYYY` is a four-digit year, no earlier than 1900.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseDate {
	#[allow(missing_docs)] pub day: NonZeroU8,
	#[allow(missing_docs)] pub month: ReleaseMonth,
	#[allow(missing_docs)] pub year: NonZeroU16,
}

impl PartialOrd for ReleaseDate {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for ReleaseDate {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		self.year.cmp(&other.year)
			.then(self.month.cmp(&other.month))
			.then(self.day.cmp(&other.day))
	}
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
	pub(crate) fn parse(mut src: &Slice32) -> Option<Self> {
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

impl fmt::Display for Release {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{} ({})", self.version, self.date)
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

impl fmt::Display for Version {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}.{:02}", self.major(), self.minor())
	}
}


impl ReleaseDate {
	pub(crate) fn parse(src: &Slice32) -> Option<Self> {
		let year = src.subslice_last(4)
			.and_then(parse_digits)
			.and_then(NonZeroU16::new)?;

		let month = src.subslice(3..6).and_then(ReleaseMonth::parse)?;

		let day  = match *src.as_ref() {
			[d, b'.', b' ', ..] // RISC OS 3.19 :(
				=> parse_digit(d),
			[_d1, _d2, b' ', ..]
				=> parse_digits(src.subslice(0..2).unwrap())
					.map(|i| i as u8), // two decimal digits always fits
			_ => None
		}.and_then(NonZeroU8::new)?;

		Some(ReleaseDate { day, month, year })
	}
}

impl fmt::Display for ReleaseDate {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:02} {} {:04}", self.day, self.month, self.year)
	}
}


impl ReleaseMonth {
	const SHORT_JAN: [u8; 3] = [b'J',b'a',b'n'];
	const SHORT_FEB: [u8; 3] = [b'F',b'e',b'b'];
	const SHORT_MAR: [u8; 3] = [b'M',b'a',b'r'];
	const SHORT_APR: [u8; 3] = [b'A',b'p',b'r'];
	const SHORT_MAY: [u8; 3] = [b'M',b'a',b'y'];
	const SHORT_JUN: [u8; 3] = [b'J',b'u',b'n'];
	const SHORT_JUL: [u8; 3] = [b'J',b'u',b'l'];
	const SHORT_AUG: [u8; 3] = [b'A',b'u',b'g'];
	const SHORT_SEP: [u8; 3] = [b'S',b'e',b'p'];
	const SHORT_OCT: [u8; 3] = [b'O',b'c',b't'];
	const SHORT_NOV: [u8; 3] = [b'N',b'o',b'v'];
	const SHORT_DEC: [u8; 3] = [b'D',b'e',b'c'];

	pub(crate) fn parse(src: &Slice32) -> Option<Self> {
		match <[u8; 3]>::try_from(src.as_ref()).ok()? {
			Self::SHORT_JAN => Some(Self::January),
			Self::SHORT_FEB => Some(Self::February),
			Self::SHORT_MAR => Some(Self::March),
			Self::SHORT_APR => Some(Self::April),
			Self::SHORT_MAY => Some(Self::May),
			Self::SHORT_JUN => Some(Self::June),
			Self::SHORT_JUL => Some(Self::July),
			Self::SHORT_AUG => Some(Self::August),
			Self::SHORT_SEP => Some(Self::September),
			Self::SHORT_OCT => Some(Self::October),
			Self::SHORT_NOV => Some(Self::November),
			Self::SHORT_DEC => Some(Self::December),
			_ => None,
		}
	}
}

impl fmt::Display for ReleaseMonth {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let short = match *self {
			Self::January => &Self::SHORT_JAN,
			Self::February => &Self::SHORT_FEB,
			Self::March => &Self::SHORT_MAR,
			Self::April => &Self::SHORT_APR,
			Self::May => &Self::SHORT_MAY,
			Self::June => &Self::SHORT_JUN,
			Self::July => &Self::SHORT_JUL,
			Self::August => &Self::SHORT_AUG,
			Self::September => &Self::SHORT_SEP,
			Self::October => &Self::SHORT_OCT,
			Self::November => &Self::SHORT_NOV,
			Self::December => &Self::SHORT_DEC,
		};

		let short = unsafe {
			// SAFETY: each array only contains printable ASCII characters
			core::str::from_utf8_unchecked(short)
		};

		fmt::Display::fmt(short, f)
	}
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

		// RISC OS 3.19 shenanigans
		assert_eq!(
			Some(ReleaseDate {
				day: NonZeroU8::new(6).unwrap(),
				month: ReleaseMonth::April,
				year: NonZeroU16::new(1472).unwrap(),
			}),
			ReleaseDate::parse(Slice32::new(b"6. Apr 1472").unwrap())
		);
	}
}

