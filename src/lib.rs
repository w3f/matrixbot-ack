#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate serde;

use actix::{prelude::*, SystemRegistry};

mod database;
mod matrix;
mod processor;
mod webhook;

type Result<T> = std::result::Result<T, anyhow::Error>;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AlertId(uuid::Uuid);

impl AlertId {
    fn new() -> Self {
        AlertId(uuid::Uuid::new_v4())
    }
}

impl AsRef<[u8]> for AlertId {
    fn as_ref(&self) -> &[u8] {
        &self.0.as_bytes()[..]
    }
}

pub fn run() -> Result<()> {
    info!("Setting up database");
    let db = database::Database::new("")?;

    info!("Adding message processor to system registry");
    let proc = processor::Processor::new(db);
    SystemRegistry::set(proc.start());

    Ok(())
}
