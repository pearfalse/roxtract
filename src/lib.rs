//! Data extraction from an Acorn-era RISC OS ROM image.
//!
//! The starting point for loading and interpreting a ROM image is the [`Rom`] struct.
#![cfg_attr(debug_assertions, allow(dead_code))]

mod heuristics;
pub use heuristics::KnownRiscOsVersion;

mod bintrinsics;
pub use bintrinsics::Slice32;
use heuristics::RomHeuristics;

use std::{
	cell::Cell,
	error::Error,
	fmt,
	io::{self, Read},
	num::NonZeroU32,
	ops::Deref,
	path::Path,
	iter::FusedIterator, borrow::Borrow,
};


type Offset = NonZeroU32;
// NonZeroU32::MAX represents 'cached find failure'
type CachedOffset = Cell<Option<Offset>>;

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

	kernel_start: CachedOffset,
	module_chain_start: CachedOffset,
	version_name_str: CachedOffset,
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

			kernel_start: CachedOffset::default(),
			module_chain_start: CachedOffset::default(),
			version_name_str: CachedOffset::default(),
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

			kernel_start: CachedOffset::default(),
			module_chain_start: CachedOffset::default(),
			version_name_str: CachedOffset::default(),
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

	fn recell_offset<F: FnOnce() -> Option<u32>>(&self, cell: &CachedOffset, find: F)
	-> Option<Offset> {
		if let cached @ Some(_) = cell.get() {
			return cached.filter(|n| *n < NonZeroU32::MAX);
		}

		let result = find().and_then(NonZeroU32::new);
		cell.set(Some(result.unwrap_or(NonZeroU32::MAX)));
		result
	}

	/// Returns the offset of the kernel in the ROM image, or `None` if it wasn't found.
	pub fn kernel_start(&self) -> Option<Offset> {
		self.recell_offset(&self.kernel_start,
			|| self.as_slice32().find(Slice32::new(b"MODULE#\0").unwrap())
			.and_then(|p| p.checked_add(8).filter(|n| *n < self.as_slice32().len())
				))
	}

	/// Returns the offset of the entry into the module chain, or `None` if `UtilityModule` wasn't
	/// found.
	pub fn module_chain_start(&self) -> Option<Offset> {
		self.recell_offset(&self.module_chain_start, ||
			self.as_slice32().find_offset_to(Slice32::new(b"UtilityModule\0").unwrap(), 0x10)
			.and_then(|n| n.checked_sub(4))
		)
	}

	/// Returns an iterator over all modules in the ROM chain.
	pub fn module_chain(&self) -> ModuleChain<'_> {
		ModuleChain::new(self, self.module_chain_start())
	}

	/// Returns a `Rom` object that transparently borrows the data of `self` as a `Slice32`.
	pub fn as_ref<'a>(&'a self) -> Rom<&'a Slice32> {
		Rom {
			data: self.as_slice32(),
			kernel_start: self.kernel_start.clone(),
			module_chain_start: self.module_chain_start.clone(),
			version_name_str: self.version_name_str.clone(),
		}
	}

	/// Returns a raw slice to the ROM image data.
	pub fn as_slice(&self) -> &[u8] {
		self.data.borrow().as_ref()
	}
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
	fn new<M: Borrow<[u8]>>(rom: &'a Rom<M>, start: Option<Offset>) -> Self {
		ModuleChain { rom: rom.as_slice32(), pos: start.map(NonZeroU32::get).unwrap_or(u32::MAX) }
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
		let offset = r.start;
		Some(Module { bytes: self.rom.subslice(r)?, offset })
	}
}

impl<'a> FusedIterator for ModuleChain<'a> { }

/// Metadata for a single module in the ROM image.
pub struct Module<'a> {
	bytes: &'a Slice32,
	offset: u32,
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
	pub const fn offset(&self) -> u32 { self.offset }
}

