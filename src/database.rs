use crate::Result;
use rocksdb::{DB, Options};

pub struct Database {
	db: DB,
}

impl Database {
	pub fn new(path: &str) -> Result<Self> {
		Ok(
			Database {
				db: DB::open_default(path)?,
			}
		)
	}
}
