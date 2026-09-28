//! Where things live on this PC.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Claude Code's own folder. It honours `CLAUDE_CONFIG_DIR`, so Pulse does too.
pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
}

/// Codex's own folder. It honours `CODEX_HOME`, so Pulse does too.
pub fn codex_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".codex"))
}

/// Pulse's settings and banked readings: `%APPDATA%\Pulse`. A fixed path
/// rather than one derived from the app handle, because `--json` reads it
/// without starting the app.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| home().join("AppData").join("Roaming"))
        .join("Pulse")
}

/// Write a file so that a crash halfway never leaves half of it behind.
pub fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}
