use std::{ffi::{OsString, OsStr}, fs, io::{Read, self}, error::Error, fmt};

use roxtract::*;

use gumdrop::Options;

#[derive(Debug, Options)]
struct CliArgs {
	#[options(free)]
	rom_path: OsString,

	#[options(help = "show help on usage")]
	help: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
	#[cfg(debug_assertions)]
	'args_override: {
		let mut args = std::env::args_os().skip(1);
		let path = match (args.next(), args.next()) {
			(Some(ref a), Some(b)) if a.as_os_str() == OsStr::new("make-crc") => b,
			_ => break 'args_override
		};

		let mut hasher = crc_any::CRCu32::crc32();
		let mut file = fs::File::open(path)?;

		const BUF_SIZE: usize = 8<<10;
		let mut buf = vec![0u8; BUF_SIZE].into_boxed_slice();
		loop {
			match file.read(&mut buf) {
				Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
				Err(e) => panic!("i/o error: {:?}", e),
				Ok(BUF_SIZE) => hasher.digest(&buf),
				Ok(part) => {
					hasher.digest(&buf[..part]);
					break
				}
			}
		};

		println!("{:08x}", hasher.get_crc());
		std::process::exit(0);
	}

	let args: CliArgs = gumdrop::parse_args_default_or_exit::<CliArgs>();

	let rom = Rom::from_file(args.rom_path)?;
	println!("Kernel starts at {:04x}", rom.kernel_start().or_print("[not found]"));
	println!("Module chain starts at {:04x}", rom.module_chain_start().or_print("[UtilityModule not found]"));

	let mut buf = String::with_capacity(40);
	for module in rom.module_chain() {
		buf.clear();
		let mod_title_pos = module.start.checked_add(0x10).and_then(|tpos| rom.read_word(tpos))
			.and_then(|rel| module.start.checked_add(rel))
			.ok_or(RomDecodeError::ModuleChainBroken)?;
		let mut i = mod_title_pos;
		loop {
			use fmt::Write;

			match rom.read_byte(i).ok_or(RomDecodeError::ModuleChainBroken)? {
				0 | b'\t' => break,
				n => write!(&mut buf, "{}", (n as char).escape_debug()).ok(),
			};
			i += 1;
		}

		print!("module: {}", &buf);
		println!();
	}

	Ok(())
}

struct HexOr<T>(Option<T>, &'static str);

impl<T: fmt::LowerHex + Copy> fmt::LowerHex for HexOr<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.0 {
			Some(ref x) => fmt::LowerHex::fmt(x, f),
			None => f.write_str(self.1),
		}
	}
}

impl<T: fmt::UpperHex + Copy> fmt::UpperHex for HexOr<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.0 {
			Some(ref x) => fmt::UpperHex::fmt(x, f),
			None => f.write_str(self.1),
		}
	}
}

trait HexOrExt {
	type Inner;
	fn or_print(self, s: &'static str) -> HexOr<Self::Inner>;
}

impl<T: fmt::LowerHex> HexOrExt for Option<T> {
	type Inner = T;
	fn or_print(self, s: &'static str) -> HexOr<T> {
		HexOr(self, s)
	}
}
