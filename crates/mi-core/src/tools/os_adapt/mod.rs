pub mod windows;

use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub title: String,
    pub process_name: String,
}

pub trait OsAdapter: Send + Sync {
    fn open_url(&self, url: &str) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
    fn open_app(
        &self,
        app: &str,
        args: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
    fn list_windows(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<WindowInfo>, String>> + Send>>;
    fn focus_window(
        &self,
        title: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
    fn close_app(
        &self,
        name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
    fn type_text(
        &self,
        text: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
    fn send_hotkey(
        &self,
        keys: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
    fn system_info(&self) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
}

pub fn create_adapter() -> Box<dyn OsAdapter> {
    Box::new(windows::WindowsAdapter)
}
