use std::{ffi::OsString, error::Error, fmt};

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
	let args: CliArgs = gumdrop::parse_args_default_or_exit::<CliArgs>();

	let rom = Rom::from_file(args.rom_path)?;
	if let Some(known) = KnownRiscOsVersion::find(&rom) {
		println!("ROM appears to be {}", known.name_high_level);
	} else {
		println!("ROM image not recognised; it may be modified or corrupted");
	}
	println!("Kernel release info (name, release): {:?}, {:?}",
		rom.os_name(), rom.heuristics().kernel_version);
	println!("Kernel starts at {:04x}", rom.heuristics().kernel_start.or_print("[not found]"));
	println!("Module chain starts at {:04x}", rom.module_chain_start()
		.ok().or_print("[UtilityModule not found]"));

	if let Ok(chain) = rom.module_chain() {
		for module in chain {
			print!("module: ");
			for ch in module.title()?.as_ref() {
				print!("{}", (*ch as char).escape_default())
			}
			println!(" (size {} bytes) at {:06x}", module.data().len(), module.offset());
		}
	} else {
		println!("could not find start of module chain");
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
