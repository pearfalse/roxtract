#![cfg_attr(debug_assertions, allow(dead_code))]

mod heuristics;

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

use crc_any::CRCu32;


type Offset = NonZeroU32;
// NonZeroU32::MAX represents 'cached find failure'
type CachedOffset = Cell<Option<Offset>>;

#[derive(Debug)]
pub enum RomLoadError {
	Io(io::Error),
	RomInvalidSize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RomDecodeError {
	UtilityModuleNotFound,
	ModuleChainBroken,
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
				=> f.write_str("ROM invalid size (mut be 2 MiB or 512KiB)"),
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


pub struct Rom<M: Borrow<[u8]> = Box<[u8]>> {
	data: M,
	crc32: u32,

	kernel_start: CachedOffset,
	module_chain_start: CachedOffset,
	version_name_str: CachedOffset,
}

const ROM_LIMIT: u32 = 12 << 20; // 12 MiB limit in the Archimedes memory map

impl Rom<Box<[u8]>> {
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

		let mut crc = CRCu32::crc32();
		crc.digest(&data);

		Ok(Rom {
			data,
			crc32: crc.get_crc(),

			kernel_start: CachedOffset::default(),
			module_chain_start: CachedOffset::default(),
			version_name_str: CachedOffset::default(),
		})
	}
}

impl<M: Borrow<[u8]>> Rom<M> {
	pub fn as_slice32(&self) -> &Slice32 {
		unsafe {
			// safety: we only allow construction of Roms <= 12MiB
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

	pub fn kernel_start(&self) -> Option<Offset> {
		self.recell_offset(&self.kernel_start,
			|| self.as_slice32().find(Slice32::new(b"MODULE#\0").unwrap())
			.and_then(|p| p.checked_add(8).filter(|n| *n < self.as_slice32().len())
				))
	}

	pub fn module_chain_start(&self) -> Option<Offset> {
		self.recell_offset(&self.module_chain_start, ||
			self.as_slice32().find_offset_to(Slice32::new(b"UtilityModule\0").unwrap(), 0x10)
			.and_then(|n| n.checked_sub(4))
		)
	}

	pub fn module_chain(&self) -> ModuleChain<'_> {
		ModuleChain::new(self, self.module_chain_start())
	}

	pub fn as_ref<'a>(&'a self) -> Rom<&'a Slice32> {
		Rom {
			data: self.as_slice32(),
			crc32: self.crc32,
			kernel_start: self.kernel_start.clone(),
			module_chain_start: self.module_chain_start.clone(),
			version_name_str: self.version_name_str.clone(),
		}
	}

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

pub struct Module<'a> {
	bytes: &'a Slice32,
	offset: u32,
}

impl<'a> Module<'a> {
	pub fn title(&self) -> Result<&Slice32, RomDecodeError> {
		self.bytes.read_word(0x10) // get title offset
			.and_then(|o| self.bytes.subslice_from(o)) // shift slice start to title start
			.and_then(Slice32::cstr) // reduce to cstr
			.ok_or(RomDecodeError::UnterminatedCstr)
	}

	#[inline]
	pub const fn data(&self) -> &Slice32 { self.bytes }

	#[inline]
	pub const fn offset(&self) -> u32 { self.offset }
}

