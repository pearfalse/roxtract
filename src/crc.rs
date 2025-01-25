use std::borrow::Borrow;

use crate::Rom;

impl<M: Borrow<[u8]>> Rom<M> {
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
}