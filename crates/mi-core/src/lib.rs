pub mod agent;
pub mod config;
pub mod events;
pub mod llm;
pub mod memory;
pub mod permissions;
pub mod secrets;
pub mod server;
pub mod tools;
pub mod vision;
pub mod voice;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
