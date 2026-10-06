//! Where things live on this PC.

use std::path::{Path, PathBuf, MAIN_SEPARATOR};

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Claude Code's own folder. It honours `CLAUDE_CONFIG_DIR`, so Pulse does too.
pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
}

/// Where Claude Code notes which account it is signed in to: beside its
/// folder, `~\.claude.json` — or, moved with `CLAUDE_CONFIG_DIR`, inside it.
/// `folder` is an account Pulse added; `None` is Claude Code's own.
pub fn claude_global_config(folder: Option<&Path>) -> PathBuf {
    match folder.map(Path::to_path_buf).or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)) {
        Some(folder) => folder.join(".claude.json"),
        None => home().join(".claude.json"),
    }
}

/// A Claude account Pulse added: a folder of Pulse's own that Claude Code is
/// pointed at, so it keeps that login there and renews it there, apart from
/// the one it is signed in to itself.
pub fn claude_account_dir(slot: &str) -> PathBuf {
    data_dir().join("accounts").join(slot)
}

/// Codex's own folder. It honours `CODEX_HOME`, so Pulse does too.
pub fn codex_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".codex"))
}

/// A path as Settings shows it, the home folder written `~`.
pub fn shown(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Pulse's settings and banked readings: `%APPDATA%\Pulse`, or
/// `%APPDATA%\Pulse Dev` for a dev copy. A fixed path rather than one derived
/// from the app handle, because `--json` reads it without starting the app.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| home().join("AppData").join("Roaming"))
        .join(crate::APP_NAME)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_at_home_is_shown_from_the_tilde() {
        let file = home().join(".claude").join(".credentials.json");
        assert_eq!(shown(&file), format!("~{MAIN_SEPARATOR}.claude{MAIN_SEPARATOR}.credentials.json"));
        // Moved elsewhere with CLAUDE_CONFIG_DIR, it is shown as it is.
        let elsewhere = PathBuf::from(if cfg!(windows) { r"D:\claude\.credentials.json" } else { "/claude/.credentials.json" });
        assert_eq!(shown(&elsewhere), elsewhere.display().to_string());
    }
}
