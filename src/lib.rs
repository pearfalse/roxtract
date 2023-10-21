#![cfg_attr(debug_assertions, allow(dead_code))]

mod heuristics;

use std::{
	cell::Cell,
	fmt,
	io::{self, Read},
	num::NonZeroU32,
	path::Path,
};

use crc_any::CRCu32;


type Offset = Cell<Option<NonZeroU32>>;

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


pub struct Rom {
	data: Box<[u8]>,
	crc32: u32,

	module_chain_start: Offset,
	version_name_str: Offset,
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
			data,
			crc32: crc.get_crc(),

			module_chain_start: Offset::default(),
			version_name_str: Offset::default(),
		})
	}
}
