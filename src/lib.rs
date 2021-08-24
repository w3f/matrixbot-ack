mod database;
mod matrix;
mod webhook;
mod processor;

type Result<T> = std::result::Result<T, anyhow::Error>;