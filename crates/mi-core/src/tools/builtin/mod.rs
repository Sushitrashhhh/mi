pub mod apps;
pub mod files;
pub mod screen;
pub mod shell;
pub mod system;
pub mod timers;
pub mod web;

use super::registry::ToolRegistry;

pub fn register_all(registry: &mut ToolRegistry) {
    system::register(registry);
    files::register(registry);
    apps::register(registry);
    shell::register(registry);
    web::register(registry);
    timers::register(registry);
    screen::register(registry);
}
