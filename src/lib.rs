#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate actix_web;
#[macro_use]
extern crate async_trait;

use actix::{prelude::*, SystemRegistry};
use actix::clock::sleep;
use std::time::Duration;

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
    fn from_bytes(slice: &[u8]) -> Result<Self> {
        Ok(AlertId(uuid::Uuid::from_slice(slice)?))
    }
}

impl AsRef<[u8]> for AlertId {
    fn as_ref(&self) -> &[u8] {
        &self.0.as_bytes()[..]
    }
}

pub async fn run() -> Result<()> {
    info!("Setting up database");
    let db = database::Database::new("")?;

    info!("Adding message processor to system registry");
    let proc = processor::Processor::new(db);
    SystemRegistry::set(proc.start());

    info!("Initializing Matrix client");
    let matrix = matrix::MatrixClient::new("", "", "", "").await?;

    info!("Adding Matrix listener to system registry");
    SystemRegistry::set(matrix.start());

    info!("Starting API server");
    webhook::run_api_server("127.0.0.1:8000").await?;

    loop {
        sleep(Duration::from_secs(u64::MAX)).await;
    }
}
