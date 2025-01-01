//! Data extraction from an Acorn-era RISC OS ROM image.
//!
//! The starting point for loading and interpreting a ROM image is the [`Rom`] struct.
#![cfg_attr(debug_assertions, allow(dead_code))]

mod heuristics;
use ascii::AsciiStr;
pub use heuristics::KnownRiscOsVersion;

mod bintrinsics;
pub use bintrinsics::Slice32;

mod release;
pub use release::{Release, Version, ReleaseDate};

use std::{
	borrow::Borrow,
	cell::Cell,
	error::Error,
	fmt,
	io::{self, Read},
	iter::FusedIterator,
	num::{NonZeroU32, NonZeroU64},
	ops::{Deref, Range},
	path::Path,
};

// NonZeroU32::MAX represents 'cached find failure'
type Offset = NonZeroU32;
type Cached<T> = Cell<Option<T>>;

/// Reasons why Roxtract will refuse to load a ROM image file.
#[derive(Debug)]
pub enum RomLoadError {
	/// The underlying device failed on an I/O operation
	Io(io::Error),
	/// The ROM is an invalid size
	RomInvalidSize,
}

/// Reasons why Roxtract cannot understand a loaded ROM image.
#[derive(Debug, PartialEq, Eq)]
pub enum RomDecodeError {
	/// UtilityModule, the start of the ROM module chain, was not found
	UtilityModuleNotFound,
	/// The module chain is broken, suggesting the ROM image is corrupted
	ModuleChainBroken,
	/// A C-string was not terminated
	UnterminatedCstr,
}

impl From<io::Error> for RomLoadError {
	fn from(value: io::Error) -> Self {
		Self::Io(value)
	}
}

impl Error for RomLoadError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			RomLoadError::Io(e) => Some(e),
			_ => None,
		}
	}
}

impl fmt::Display for RomLoadError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			RomLoadError::Io(e)
				=> write!(f, "I/O error: {}", e),
			RomLoadError::RomInvalidSize
				=> f.write_str("ROM invalid size (mut be 32-bit aligned and no more than 12 MiB)"),
		}
	}
}

impl fmt::Display for RomDecodeError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			RomDecodeError::UtilityModuleNotFound
				=> f.write_str("Could not find UtilityModule in ROM (is file corrupted?)"),
			RomDecodeError::ModuleChainBroken
				=> f.write_str("Module chain appears to be broken"),
			RomDecodeError::UnterminatedCstr
				=> f.write_str("C-string terminator could not be located"),
		}
	}
}

impl Error for RomDecodeError { }


/// A wrapper round a RISC OS ROM image.
///
/// A RISC OS ROM image is considered to have the following parts:
///
/// - Entry point and bootloader;
/// - Kernel;
/// - Chain (linked list) of built-in modules, starting with `UtilityModule`;
/// - Padding (and unknown trailing data in the last 12 bytes).
///
/// The ROM image has to be contiguous in system memory.
pub struct Rom<M: Borrow<[u8]> = Box<[u8]>> {
	data: M,

	kernel_start: Cached<Offset>,
	kernel_version_str_pos: Cached<NonZeroU32>,
	kernel_version: Cached<Release>,
	module_chain_start: Cached<Offset>,
	version_name_str: Cached<Offset>,
	crc32_hash: Cached<u32>,
	sort_key: Cached<NonZeroU64>,
	publisher_range: Cached<Range<NonZeroU32>>,
}

const ROM_LIMIT: u32 = 12 << 20; // 12 MiB limit in the Archimedes memory map

impl Rom<Box<[u8]>> {
	/// Creates a `Rom` owning its contents from a file.
	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, RomLoadError> {
		Self::from_file_impl(path.as_ref())
	}

	fn from_file_impl(path: &Path) -> Result<Self, RomLoadError> {
		let mut file = std::fs::File::open(path)?;

		let rom_len = match file.metadata()?.len() {
			// small enough and word-aligned?
			n if n <= ROM_LIMIT as u64 && n & 3 == 0 => n as u32,
			_ => return Err(RomLoadError::RomInvalidSize),
		};

		let mut data = vec![0u8; rom_len as usize].into_boxed_slice();
		file.read_exact(&mut data)?;

		Ok(Rom {
			data,

			kernel_start: Cached::default(),
			kernel_version_str_pos: Cached::default(),
			kernel_version: Cached::default(),
			module_chain_start: Cached::default(),
			version_name_str: Cached::default(),
			crc32_hash: Cached::default(),
			sort_key: Cached::default(),
			publisher_range: Cached::default()
		})
	}
}

impl<M: Borrow<[u8]>> Rom<M> {
	/// Creates a `Rom` from some existing memory allocation containing a ROM image.
	pub fn from_mem(mem: M) -> Result<Rom<M>, RomLoadError> {
		let data = mem.borrow();
		if data.len() > ROM_LIMIT as usize || data.len() & 3 != 0 {
			return Err(RomLoadError::RomInvalidSize);
		}

		Ok(Rom {
			data: mem,

			kernel_start: Cached::default(),
			kernel_version_str_pos: Cached::default(),
			kernel_version: Cached::default(),
			module_chain_start: Cached::default(),
			version_name_str: Cached::default(),
			crc32_hash: Cached::default(),
			sort_key: Cached::default(),
			publisher_range: Cached::default()
		})
	}
}

impl<M: Borrow<[u8]>> Rom<M> {
	/// Returns a slice of the ROM image.
	#[inline]
	pub fn as_slice32(&self) -> &Slice32 {
		unsafe {
			// SAFETY: we only allow construction of Roms <= 12 MiB
			// so Slice32 will hold them no problem
			Slice32::new_unchecked(self.data.borrow())
		}
	}

	fn recell_offset<T: Recell, F: FnOnce() -> Option<T>>(&self, cell: &Cached<T>, find: F)
	-> Option<T> {
		// TODO: this needs to support `find` coercing `None` to `::MAX`
		if let cached @ Some(_) = cell.get() {
			return cached.filter(|n| *n != T::FIND_FAILURE);
		}

		let result = find();
		cell.set(Some(result.unwrap_or(T::FIND_FAILURE)));
		result
	}

	/// Returns the offset of the kernel in the ROM image, or `None` if it wasn't found.
	pub fn kernel_start(&self) -> Option<Offset> {
		let is_in_range = {
			let len = self.as_slice32().len();
			move |pos: &u32| *pos < len
		};

		self.recell_offset(&self.kernel_start, || {
			let mut end_of_module0 = self.as_slice32().find(Slice32::new(b"MODULE#\0").unwrap())
				.and_then(|p| p.checked_add(8).filter(is_in_range))?;

			// Arthur 0.x up through RISC OS 2.00 have two extra non-zero words between `MODULE#0\0`
			// and the faux-module header; skip over them if present
			let mut word_skip = 2;
			while word_skip > 0 && self.as_slice32().read_word(end_of_module0)? != 0 {
				end_of_module0 = end_of_module0.checked_add(4).filter(is_in_range)?;
				word_skip -= 1;
			}

			NonZeroU32::new(end_of_module0)
		})
	}

	fn kernel_version_str_pos(&self) -> Option<NonZeroU32> {
		self.recell_offset(&self.kernel_version_str_pos, || {
			let kernel_start = self.kernel_start()?;
			let kernel_title_offset = kernel_start.checked_add(0x14) // title offset
				.and_then(|o| self.as_slice32().read_word(o.get()))
				.and_then(NonZeroU32::new)
				?;

			kernel_start.checked_add(kernel_title_offset.get())
		})
	}

	/// Returns a byte slice to the kernel version string (usually of the form
	/// `{OS name}\t\tV.VV (DD Mmm YYYY)`).
	pub fn kernel_version_str(&self) -> Option<&Slice32> {
		self.kernel_version_str_pos()
			.and_then(|pos| self.as_slice32().subslice_from(pos.get()))
			.and_then(Slice32::cstr)
	}

	/// Returns the OS name (likely 'RISC OS' or 'Arthur').
	pub fn os_name(&self) -> Option<&AsciiStr> {
		use ascii::AsAsciiStr as _;

		let basis = self.kernel_version_str()?;
		let first_tab_at = basis.index_of(b'\t')?;
		basis.subslice(0..first_tab_at)?.as_ref().as_ascii_str().ok()
	}

	/// Returns the kernel release information.
	pub fn kernel_version(&self) -> Option<Release> {
		self.recell_offset(&self.kernel_version, ||
			self.kernel_version_str().and_then(Release::parse))
	}

	/// Returns the offset of the entry into the module chain, or `None` if `UtilityModule` wasn't
	/// found.
	pub fn module_chain_start(&self) -> Result<Offset, RomDecodeError> {
		let found = self.recell_offset(&self.module_chain_start, ||
			self.as_slice32().find_offset_to(Slice32::new(b"UtilityModule\0").unwrap(), 0x10)
			.and_then(|n| n.checked_sub(4))
			.and_then(NonZeroU32::new)
		);

		found.ok_or(RomDecodeError::UtilityModuleNotFound)
	}

	/// Returns the CRC32 hash of the ROM image.
	pub fn crc32_hash(&self) -> u32 {
		if let Some(already) = self.crc32_hash.get() { return already; }

		let hash = calc_hash(self.data.borrow());
		#[inline(never)] // the CRC types consume a lot of stack space
		fn calc_hash(data: &[u8]) -> u32 {
			let mut hasher = crc_any::CRCu32::crc32();
			hasher.digest(data);
			hasher.get_crc()
		}
		self.crc32_hash.set(Some(hash));
		hash
	}

	/// Returns an integer key that can be used to sort multiple ROM images by version.
	pub fn sort_key(&self) -> NonZeroU64 {
		if let Some(already) = self.sort_key.get() { return already; }

		let key = calc_sort_key(self).unwrap_or(NonZeroU64::MAX);
		self.sort_key.set(Some(key));
		key
	}

	/// Returns an iterator over all modules in the ROM chain.
	pub fn module_chain(&self) -> Result<ModuleChain<'_>, RomDecodeError> {
		self.module_chain_start().map(|addr| ModuleChain::new(self, addr))
	}

	/// Returns a `Rom` object that transparently borrows the data of `self` as a `Slice32`.
	pub fn as_ref<'a>(&'a self) -> Rom<&'a Slice32> {
		Rom {
			data: self.as_slice32(),
			kernel_start: self.kernel_start.clone(),
			kernel_version_str_pos: self.kernel_start.clone(),
			kernel_version: self.kernel_version.clone(),
			module_chain_start: self.module_chain_start.clone(),
			version_name_str: self.version_name_str.clone(),
			crc32_hash: self.crc32_hash.clone(),
			sort_key: self.sort_key.clone(),
			publisher_range: self.publisher_range.clone(),
		}
	}

	/// Returns a raw slice to the ROM image data.
	pub fn as_slice(&self) -> &[u8] {
		self.data.borrow().as_ref()
	}
}

trait Clone2 : Sized {
	fn clone(&self) -> Self;
}

impl<T: Clone> Clone2 for Cached<Range<T>> {
	fn clone(&self) -> Self {
		let as_ref = unsafe {
			// SAFETY: we won't mutate the original cell, so taking a shared ref to its contents is
			// fine
			&*self.as_ptr()
		}.as_ref();

		Cell::new(as_ref.cloned())
	}
}

trait Recell : Copy + Eq {
	const FIND_FAILURE: Self;
}

impl Recell for NonZeroU32 {
	const FIND_FAILURE: Self = NonZeroU32::MAX;
}

impl Recell for NonZeroU64 {
	const FIND_FAILURE: Self = NonZeroU64::MAX;
}

// vvvYYYMMdd where
// vvv = decimalised version, à la Wimp_Initialise (e.g. RISC OS 3.11 := 311, Arthur 0.30 := 30)
// YYY = year of release - 1900 (e.g. RISC OS 2.01 := 90)
// MM = month of release (01..=12)
// dd = date of release (01..=31)
#[inline(never)]
fn calc_sort_key<M: Borrow<[u8]>>(rom: &Rom<M>) -> Option<NonZeroU64> {
	rom.kernel_version().map(calc_sort_key_2)
}

#[inline(never)]
#[allow(clippy::inconsistent_digit_grouping)]
fn calc_sort_key_2(release: Release) -> NonZeroU64 {
	let version_int = release.version.major() as u64 * 100 + release.version.minor() as u64;

	let date_int = (release.date.year().get() as u64).saturating_sub(1900).min(999) * 1_00_00
		+
		release.date.month() as u64 * 100
		+
		release.date.day().get() as u64;

	let full = version_int * 1_000_00_00 + date_int;
	debug_assert!(full != 0);
	NonZeroU64::new(full).unwrap()
}

impl Deref for Rom {
	type Target = Slice32;

	fn deref(&self) -> &Self::Target {
		self.as_slice32()
	}
}

impl<M: Borrow<[u8]>> Borrow<[u8]> for Rom<M> {
	#[inline]
	fn borrow(&self) -> &[u8] {
		self.data.borrow()
	}
}

impl<M: Borrow<[u8]>> Borrow<Slice32> for Rom<M> {
	#[inline]
	fn borrow(&self) -> &Slice32 {
		self.as_slice32()
	}
}

/// An iterator over each module in the ROM image.
pub struct ModuleChain<'a> {
	rom: &'a Slice32,
	pos: u32,
}

impl<'a> ModuleChain<'a> {
	fn new<M: Borrow<[u8]>>(rom: &'a Rom<M>, start: Offset) -> Self {
		ModuleChain { rom: rom.as_slice32(), pos: start.get() }
	}

	#[inline]
	fn in_range(&self) -> impl Fn(&u32) -> bool {
		let len = self.rom.len();
		move |n| *n < len
	}
}

impl<'a> Iterator for ModuleChain<'a> {
	type Item = Module<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let (module_start, module_len) = (
			self.pos.checked_add(4)?, self.rom.read_word(self.pos)?
		);

		if module_len > 0 {
			self.pos = self.pos.checked_add(module_len)
				.filter(self.in_range())
				.unwrap_or(u32::MAX);
		} else {
			self.pos = u32::MAX;
			return None;
		}

		// sub 4 to remove chain length word (`module_len` includes this)
		let r = module_start .. module_start.checked_sub(4)?.saturating_add(module_len);
		let Some(offset) = NonZeroU32::new(r.start) else {
			debug_assert!(false, "this should never be 0!");
			self.pos = u32::MAX;
			return None;
		};
		Some(Module { bytes: self.rom.subslice(r)?, offset })
	}
}

impl<'a> FusedIterator for ModuleChain<'a> { }

/// Metadata for a single module in the ROM image.
pub struct Module<'a> {
	bytes: &'a Slice32,
	offset: NonZeroU32,
}

impl<'a> Module<'a> {
	/// Returns a slice over the C-string of this module title.
	pub fn title(&self) -> Result<&Slice32, RomDecodeError> {
		self.bytes.read_word(0x10) // get title offset
			.and_then(|o| self.bytes.subslice_from(o)) // shift slice start to title start
			.and_then(Slice32::cstr) // reduce to cstr
			.ok_or(RomDecodeError::UnterminatedCstr)
	}

	/// Returns a slice over the entire module contents.
	#[inline]
	pub const fn data(&self) -> &Slice32 { self.bytes }

	/// Returns the offset of this module within the ROM image.
	#[inline]
	pub const fn offset(&self) -> NonZeroU32 { self.offset }
}

#[cfg(test)]
mod test {
    use std::num::{NonZeroU32, NonZeroU64};

    use crate::Slice32;
    use assert_hex::assert_eq_hex;

	static ROM_TEST1: &[u8] = include_bytes!("../testrom1");
    static ROM_RO3: &[u8] = include_bytes!("../testrom_kernelonly");
    static ROM_RO200_KERNEL: &[u8] = include_bytes!("../testrom_ro200_words");

	#[test]
	fn sort_key() {
		for (expect, from) in [
			(Some(3110920929), b"RISC OS\t\t3.11 (29 Sep 1992)".as_slice()),
			(Some(9891231122), b"RISC OS\t\t9.89 (22 Nov 2023)"),
			(Some(2970501001), b"Unnamed German OS\t\t2.97 (1 Oct 1950)"), // no leading date 0

			(None, b"No tabs"),
			(None, b"Just one tab\t1.23 (11 Jan 2000)"),
			(None, b"No open paren\t\t1.11 23 Feb 2001"),
			(None, b"Truncated\t\t1.00 (01 Jan 2000"),
		] {
			let result = Slice32::new(from).and_then(crate::Release::parse)
				.map(|r| super::calc_sort_key_2(r).get());
			assert_eq!(expect, result);
		}
	}

	#[test]
	fn test_rom_1() {

		let rom = super::Rom::from_mem(ROM_TEST1).unwrap();
		assert_eq_hex!(NonZeroU32::new(0x20), rom.kernel_start());
		assert_eq_hex!(NonZeroU32::new(0x5c), rom.module_chain_start().ok());

		let mut modules = rom.module_chain().unwrap();
		let module = modules.next().unwrap();

		assert_eq_hex!(Some(b"UtilityModule".as_slice()), module.title().ok().map(AsRef::as_ref));
		assert_eq_hex!(NonZeroU32::new(0x60).unwrap(), module.offset());

		let module = modules.next().unwrap();
		assert_eq_hex!(Some(b"Module2".as_slice()), module.title().ok().map(AsRef::as_ref));
		assert_eq_hex!(NonZeroU32::new(0xb4).unwrap(), module.offset());

		let module = modules.next().unwrap();
		assert_eq_hex!(Some(b"Module3".as_slice()), module.title().ok().map(AsRef::as_ref));
		assert_eq_hex!(NonZeroU32::new(0xdc).unwrap(), module.offset());
	}

	#[test]
	fn extract_version_from_kernel() {
		let cases = [
			(
				ROM_RO3,
				Some(&b"RISC OS\t\t3.45 (28 Feb 2084)"[..]),
				Some(&b"RISC OS"[..]),
				3451840228,
			),
			(
				ROM_RO200_KERNEL,
				Some(b"Arthur\t\t0.66 (02 Feb 2022)"),
				Some(b"Arthur"),
				0661220202,
			),
		];

		for (rom_data, full_ver, name, key) in cases {
			let rom = super::Rom::from_mem(rom_data).unwrap();

			assert_eq!(full_ver, rom.kernel_version_str().map(Slice32::as_ref));
			assert_eq!(name, rom.os_name().map(ascii::AsciiStr::as_bytes));

			assert_eq!(NonZeroU64::new(key),
				Some(rom.sort_key()).filter(|n| *n != NonZeroU64::MAX));
		}
	}
}
