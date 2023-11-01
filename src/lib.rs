#![cfg_attr(debug_assertions, allow(dead_code))]

mod heuristics;
use bintrinsics::Slice32;
pub use heuristics::SliceExt;

mod bintrinsics;

use std::{
	cell::Cell,
	error::Error,
	fmt,
	io::{self, Read},
	num::NonZeroU32,
	ops::{Range, Deref},
	path::Path,
	iter::FusedIterator,
};

use crc_any::CRCu32;


type Offset = Option<NonZeroU32>;
type CachedOffset = Cell<Option<Offset>>;

#[derive(Debug)]
pub enum RomLoadError {
	Io(io::Error),
	RomInvalidSize,
}

#[derive(Debug)]
pub enum RomDecodeError {
	UtilityModuleNotFound,
	ModuleChainBroken,
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
		}
	}
}

impl Error for RomDecodeError { }


pub struct Rom {
	data: Box<Slice32>,
	crc32: u32,

	kernel_start: CachedOffset,
	module_chain_start: CachedOffset,
	version_name_str: CachedOffset,
}

impl Rom {
	const RISC_OS_2_LEN: u32 = 512 << 10; // 512 KiB
	const RISC_OS_3_LEN: u32 = 2 << 20; // 2 MiB

	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Rom, RomLoadError> {
		Self::from_file_impl(path.as_ref())
	}

	fn from_file_impl(path: &Path) -> Result<Rom, RomLoadError> {
		let mut file = std::fs::File::open(path)?;

		let rom_len = match file.metadata()?.len() {
			n if n == Self::RISC_OS_2_LEN as u64 || n == Self::RISC_OS_3_LEN as u64
				=> n as u32,
			_ => return Err(RomLoadError::RomInvalidSize),
		};

		let mut data = vec![0u8; rom_len as usize].into_boxed_slice();
		file.read_exact(&mut data)?;

		let mut crc = CRCu32::crc32();
		crc.digest(&data);

		Ok(Rom {
			data: Slice32::new_boxed(data).unwrap(),
			crc32: crc.get_crc(),

			kernel_start: CachedOffset::default(),
			module_chain_start: CachedOffset::default(),
			version_name_str: CachedOffset::default(),
		})
	}

	fn in_range(&self) -> impl Fn(&u32) -> bool {
		let len = self.data.len() as u32;
		move |n| *n < len
	}

	fn recell_offset<F: FnOnce() -> Option<u32>>(&self, cell: &CachedOffset, find: F) -> Offset {
		if let Some(cached) = cell.get() { return cached; }

		let result = find().and_then(NonZeroU32::new);
		cell.set(Some(result));
		result
	}

	pub fn kernel_start(&self) -> Offset {
		self.recell_offset(&self.kernel_start,
			|| Self::find(self.data.as_ref(), Slice32::new(b"MODULE#\0").unwrap())
			.and_then(|p| p.checked_add(8).filter(self.in_range())
				))
	}

	pub fn module_chain_start(&self) -> Offset {
		self.recell_offset(&self.module_chain_start, || Self::find_offset_to(
			self.data.as_ref(), Slice32::new(b"UtilityModule\0").unwrap(), 0x10)
			.and_then(|n| n.checked_sub(4)))
	}

	pub fn module_chain(&self) -> ModuleChain<'_> {
		ModuleChain::new(self, self.module_chain_start())
	}
}

impl Deref for Rom {
	type Target = Slice32;

	fn deref(&self) -> &Self::Target {
		self.data.as_ref()
	}
}


pub struct ModuleChain<'a> {
	rom: &'a Rom,
	pos: u32,
}

impl<'a> ModuleChain<'a> {
	fn new(rom: &'a Rom, start: Offset) -> Self {
		ModuleChain { rom, pos: start.map(NonZeroU32::get).unwrap_or(u32::MAX) }
	}
}

impl<'a> Iterator for ModuleChain<'a> {
	type Item = Range<u32>;

	fn next(&mut self) -> Option<Self::Item> {
		let (module_start, module_len) = (
			self.pos.checked_add(4)?, self.rom.data.get_word(self.pos as usize)?
		);

		if module_len > 0 {
			self.pos = self.pos.checked_add(module_len)
				.filter(self.rom.in_range())
				.unwrap_or(u32::MAX);
		} else {
			self.pos = u32::MAX;
			return None;
		}
		Some(module_start .. module_start.saturating_add(module_len))
	}
}

impl<'a> FusedIterator for ModuleChain<'a> { }

