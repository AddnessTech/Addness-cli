use std::fs::OpenOptions;
use std::io::Write;

/// Debug logging to file (TUI doesn't capture this)
pub fn debug_log(msg: &str) {
    let home = dirs::home_dir().unwrap();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join(".addness").join("debug.log"))
        .unwrap();
    writeln!(
        file,
        "[{}] {}",
        chrono::Utc::now().format("%H:%M:%S%.3f"),
        msg
    )
    .ok();
}

#[macro_export]
macro_rules! dbg_log {
    ($($arg:tt)*) => {
        $crate::debug_log::debug_log(&format!($($arg)*))
    };
}
