//! Where things live on this PC.

use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::sync::OnceLock;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Claude Code's own folder. It honours `CLAUDE_CONFIG_DIR`, so Dipstick does too.
pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
}

/// Where Claude Code notes which account it is signed in to: beside its
/// folder, `~\.claude.json` — or, moved with `CLAUDE_CONFIG_DIR`, inside it.
/// `folder` is an account Dipstick added; `None` is Claude Code's own.
pub fn claude_global_config(folder: Option<&Path>) -> PathBuf {
    match folder.map(Path::to_path_buf).or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)) {
        Some(folder) => folder.join(".claude.json"),
        None => home().join(".claude.json"),
    }
}

/// A Claude account Dipstick added: a folder of Dipstick's own that Claude Code is
/// pointed at, so it keeps that login there and renews it there, apart from
/// the one it is signed in to itself.
pub fn claude_account_dir(slot: &str) -> PathBuf {
    data_dir().join("accounts").join(slot)
}

/// Codex's own folder. It honours `CODEX_HOME`, so Dipstick does too.
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

/// Dipstick's settings and banked readings: `%APPDATA%\Dipstick`, or
/// `%APPDATA%\Dipstick Dev` for a dev copy. A fixed path rather than one derived
/// from the app handle, because `--json` reads it without starting the app.
///
/// Decided once a run: the first call moves Pulse's folder here (`settle`).
/// Tests get a folder of their own, so they never write to an app's
/// folder, or move one.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        if cfg!(test) {
            return std::env::temp_dir().join(format!("dipstick-test-{}", std::process::id()));
        }
        settle(&roaming(), crate::OLD_APP_NAME, crate::APP_NAME)
    })
    .clone()
}

/// Where Pulse kept the same things, before it was renamed Dipstick.
pub fn old_data_dir() -> PathBuf {
    roaming().join(crate::OLD_APP_NAME)
}

fn roaming() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join("AppData").join("Roaming"))
}

/// The folder named `new` in `root`, Pulse's `old` one moved to it if that
/// is all there is. Moved with one rename and never copied: the accounts'
/// logins are in it, and renewing one copy of a login leaves the other copy
/// useless. A folder that won't move (a program has something in it open)
/// is used where it is, and the next start tries again.
fn settle(root: &Path, old: &str, new: &str) -> PathBuf {
    let (old, new) = (root.join(old), root.join(new));
    if new.exists() || !old.is_dir() {
        return new;
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => new,
        Err(_) => old,
    }
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

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dipstick-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn pulses_folder_is_moved_not_copied() {
        let root = scratch("moved");
        let login = root.join("Pulse").join("accounts").join("a1").join(".credentials.json");
        std::fs::create_dir_all(login.parent().unwrap()).unwrap();
        std::fs::write(&login, "login").unwrap();

        assert_eq!(settle(&root, "Pulse", "Dipstick"), root.join("Dipstick"));
        let moved = root.join("Dipstick").join("accounts").join("a1").join(".credentials.json");
        assert_eq!(std::fs::read_to_string(moved).unwrap(), "login");
        assert!(!root.join("Pulse").exists());
        // Started again, it stays.
        assert_eq!(settle(&root, "Pulse", "Dipstick"), root.join("Dipstick"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_of_its_own_is_never_replaced() {
        let root = scratch("own");
        std::fs::create_dir_all(root.join("Pulse")).unwrap();
        std::fs::write(root.join("Pulse").join("settings.json"), "pulse").unwrap();
        std::fs::create_dir_all(root.join("Dipstick")).unwrap();
        std::fs::write(root.join("Dipstick").join("settings.json"), "dipstick").unwrap();

        assert_eq!(settle(&root, "Pulse", "Dipstick"), root.join("Dipstick"));
        assert_eq!(std::fs::read_to_string(root.join("Dipstick").join("settings.json")).unwrap(), "dipstick");
        assert_eq!(std::fs::read_to_string(root.join("Pulse").join("settings.json")).unwrap(), "pulse");

        // With no Pulse at all, nothing is made yet.
        let fresh = scratch("fresh");
        assert_eq!(settle(&fresh, "Pulse", "Dipstick"), fresh.join("Dipstick"));
        assert!(!fresh.join("Dipstick").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&fresh);
    }

    /// Something in it open, as a program still running from it would have:
    /// used where it is, then moved once it is let go.
    #[cfg(windows)]
    #[test]
    fn a_folder_in_use_is_used_where_it_is() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = scratch("in-use");
        std::fs::create_dir_all(root.join("Pulse")).unwrap();
        let file = root.join("Pulse").join("settings.json");
        std::fs::write(&file, "pulse").unwrap();
        let open = std::fs::OpenOptions::new().read(true).share_mode(0).open(&file).unwrap();

        assert_eq!(settle(&root, "Pulse", "Dipstick"), root.join("Pulse"));
        assert!(!root.join("Dipstick").exists());

        drop(open);
        assert_eq!(settle(&root, "Pulse", "Dipstick"), root.join("Dipstick"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
