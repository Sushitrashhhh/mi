use std::io;
use std::net::TcpListener;
use std::path::PathBuf;

pub struct InstanceLock {
    _listener: TcpListener,
    pid_file: PathBuf,
}

impl InstanceLock {
    pub fn acquire() -> io::Result<Self> {
        let mi_dir = dirs::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?
            .join(".mi");
        let pid_path = mi_dir.join("mi.pid");

        let listener = TcpListener::bind("127.0.0.1:17776").map_err(|_| {
            io::Error::new(
                io::ErrorKind::AddrInUse,
                "another mi instance is already running",
            )
        })?;

        let _ = std::fs::write(&pid_path, std::process::id().to_string());

        Ok(Self {
            _listener: listener,
            pid_file: pid_path,
        })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.pid_file);
    }
}
