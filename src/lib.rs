pub mod config;
pub mod data;
pub mod error;
pub mod exchange;
pub mod git;
pub mod index;
pub mod memory;
pub mod migration;
pub mod models;
pub mod project;
pub mod results;
pub mod store;
pub mod tools;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FORMAT: u32 = 1;
