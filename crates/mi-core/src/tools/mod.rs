pub mod builtin;
pub mod os_adapt;
pub mod registry;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use builtin::timers::TimerStore;
use os_adapt::OsAdapter;
use registry::ToolResult;

pub struct ToolExecutor {
    pub os: Arc<dyn OsAdapter>,
    pub quarantine_dir: PathBuf,
    pub timers: Arc<Mutex<TimerStore>>,
}

impl ToolExecutor {
    pub fn new(os: Box<dyn OsAdapter>, quarantine_dir: PathBuf) -> Self {
        Self {
            os: Arc::from(os),
            quarantine_dir,
            timers: Arc::new(Mutex::new(TimerStore::new())),
        }
    }

    pub async fn execute(&self, name: &str, args: serde_json::Value) -> ToolResult {
        match name {
            "system.time" => builtin::system::time(),
            "system.info" => builtin::system::info(&*self.os).await,
            "files.list" => builtin::files::list(args).await,
            "files.read" => builtin::files::read(args).await,
            "files.create" => builtin::files::create(args).await,
            "files.move" => builtin::files::r#move(args).await,
            "files.delete" => builtin::files::delete(args, &self.quarantine_dir).await,
            "apps.list" => builtin::apps::list(&*self.os).await,
            "apps.open" => builtin::apps::open(args, &*self.os).await,
            "apps.focus" => builtin::apps::focus(args, &*self.os).await,
            "apps.close" => builtin::apps::close(args, &*self.os).await,
            "apps.type_text" => builtin::apps::type_text(args, &*self.os).await,
            "apps.hotkey" => builtin::apps::hotkey(args, &*self.os).await,
            "shell.run" => builtin::shell::run(args).await,
            "web.search" => builtin::web::search(args).await,
            "web.fetch" => builtin::web::fetch(args).await,
            "timers.set" => builtin::timers::set(args, &self.timers).await,
            "timers.list" => builtin::timers::list(&self.timers).await,
            "timers.cancel" => builtin::timers::cancel(args, &self.timers).await,
            "screen.capture" => builtin::screen::capture_tool(args).await,
            _ => ToolResult::error(format!("unknown tool: {}", name)),
        }
    }
}
