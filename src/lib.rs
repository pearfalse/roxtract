#![cfg_attr(debug_assertions, allow(dead_code))]

mod heuristics;
use bintrinsics::Slice32;

mod bintrinsics;

use std::{
	cell::Cell,
	error::Error,
	fmt,
	io::{self, Read},
	num::NonZeroU32,
	ops::Deref,
	path::Path,
	iter::FusedIterator,
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

#[derive(Debug)]
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
			|| Self::find(self.data.as_ref(), Slice32::new(b"MODULE#\0").unwrap())
			.and_then(|p| p.checked_add(8).filter(self.in_range())
				))
	}

	pub fn module_chain_start(&self) -> Option<Offset> {
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
	fn new(rom: &'a Rom, start: Option<Offset>) -> Self {
		ModuleChain { rom, pos: start.map(NonZeroU32::get).unwrap_or(u32::MAX) }
	}
}

impl<'a> Iterator for ModuleChain<'a> {
	type Item = Module<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let (module_start, module_len) = (
			self.pos.checked_add(4)?, self.rom.data.read_word(self.pos)?
		);

		if module_len > 0 {
			self.pos = self.pos.checked_add(module_len)
				.filter(self.rom.in_range())
				.unwrap_or(u32::MAX);
		} else {
			self.pos = u32::MAX;
			return None;
		}

		// sub 4 to remove chain length word (`module_len` includes this)
		let r = module_start .. module_start.checked_sub(4)?.saturating_add(module_len);
		Some(Module { bytes: self.rom.data.subslice(r)? })
	}
}

impl<'a> FusedIterator for ModuleChain<'a> { }

pub struct Module<'a> {
	bytes: &'a Slice32,
}

impl<'a> Module<'a> {
	pub fn title(&self) -> Result<&Slice32, RomDecodeError> {
		self.bytes.read_word(0x10) // get title offset
			.and_then(|o| self.bytes.subslice_from(o)) // shift slice start to title start
			.and_then(Slice32::cstr) // reduce to cstr
			.ok_or(RomDecodeError::UnterminatedCstr)
	}
}

