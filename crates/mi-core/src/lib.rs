pub mod config;
pub mod events;
pub mod llm;
pub mod secrets;
pub mod server;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
