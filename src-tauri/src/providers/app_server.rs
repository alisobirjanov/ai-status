//! A JSON-RPC client for `codex app-server`, Codex's own documented protocol.
//!
//! The macOS app keeps the helper resident. Here it is started per request
//! and taken down with its whole process tree afterwards: it is only the
//! fallback for a refused token, and a helper that is never left running can
//! never be left spinning on a closed pipe (macOS issue #25).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::paths;

#[derive(Debug)]
pub enum AppServerError {
    /// Codex isn't installed, or isn't anywhere we thought to look.
    NotFound,
    StartFailed,
    TimedOut,
    Server(String),
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// No console window flashes up for a child of a GUI app.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The account's limits, as `account/rateLimits/read` reports them.
pub async fn rate_limits() -> Result<Value, AppServerError> {
    let executable = locate().ok_or(AppServerError::NotFound)?;

    let mut command = tokio::process::Command::new(&executable);
    command
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // The folder `codex` was found in leads the helper's PATH: an npm
    // install is a Node script, and `node` lives beside it.
    if let Some(folder) = executable.parent() {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let mut leading = std::ffi::OsString::from(folder.as_os_str());
        leading.push(";");
        leading.push(inherited);
        command.env("PATH", leading);
    }
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn().map_err(|_| AppServerError::StartFailed)?;
    let pid = child.id();

    let conversation = async {
        let mut stdin = child.stdin.take().ok_or(AppServerError::StartFailed)?;
        let stdout = child.stdout.take().ok_or(AppServerError::StartFailed)?;
        let mut lines = BufReader::new(stdout).lines();

        // The protocol opens with a handshake before anything else is accepted.
        send(&mut stdin, json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "clientInfo": { "name": "Pulse", "title": "Pulse", "version": env!("CARGO_PKG_VERSION") } }
        }))
        .await?;
        answer(&mut lines, 1).await?;
        send(&mut stdin, json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} })).await?;
        send(&mut stdin, json!({ "jsonrpc": "2.0", "id": 2, "method": "account/rateLimits/read", "params": {} })).await?;
        answer(&mut lines, 2).await
    };

    let result = match tokio::time::timeout(REQUEST_TIMEOUT, conversation).await {
        Ok(result) => result,
        Err(_) => Err(AppServerError::TimedOut),
    };

    shut_down(&mut child, pid).await;
    result
}

async fn send(stdin: &mut tokio::process::ChildStdin, message: Value) -> Result<(), AppServerError> {
    let mut line = serde_json::to_vec(&message).map_err(|_| AppServerError::StartFailed)?;
    line.push(b'\n');
    // A write into a pipe whose far end has gone is an error here, not a
    // signal that kills the process: Windows has no SIGPIPE.
    stdin.write_all(&line).await.map_err(|_| AppServerError::StartFailed)?;
    stdin.flush().await.map_err(|_| AppServerError::StartFailed)
}

/// Newline-delimited JSON. Anything that is not the reply being waited for —
/// a notification, a line that is not JSON — is passed over. EOF means the
/// helper has gone, and nothing will answer.
async fn answer(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: i64,
) -> Result<Value, AppServerError> {
    loop {
        let line = lines
            .next_line()
            .await
            .map_err(|_| AppServerError::StartFailed)?
            .ok_or(AppServerError::StartFailed)?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            let text = error.get("message").and_then(Value::as_str).unwrap_or("unknown");
            return Err(AppServerError::Server(text.to_string()));
        }
        return Ok(message.get("result").cloned().unwrap_or_else(|| json!({})));
    }
}

/// The whole tree, not only the process started: an npm shim is `cmd` →
/// `node` → `codex.exe`, and killing the first leaves the other two behind.
async fn shut_down(child: &mut tokio::process::Child, pid: Option<u32>) {
    #[cfg(windows)]
    if let Some(pid) = pid {
        let mut taskkill = tokio::process::Command::new("taskkill");
        taskkill
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let _ = taskkill.status().await;
    }
    #[cfg(not(windows))]
    let _ = pid;
    let _ = child.kill().await;
}

// MARK: - Finding codex

/// The native binary where one can be found, since it needs no `node` and
/// dies with one kill; an npm shim otherwise.
fn locate() -> Option<PathBuf> {
    let folders = search_folders();
    native_candidates(&folders)
        .into_iter()
        .chain(folders.iter().map(|f| f.join("codex.cmd")))
        .find(|candidate| candidate.is_file())
}

/// PATH first, then where the usual installers put things. A GUI app on
/// Windows inherits the user's PATH, but a login-item launch can predate a
/// change to it, so the common places are listed too.
fn search_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();

    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let home = paths::home();
    folders.extend(
        [
            env("APPDATA").map(|a| a.join("npm")),
            env("NVM_SYMLINK"),
            env("ProgramFiles").map(|p| p.join("nodejs")),
            env("LOCALAPPDATA").map(|l| l.join("pnpm")),
            env("LOCALAPPDATA").map(|l| l.join("Volta").join("bin")),
            env("LOCALAPPDATA").map(|l| l.join("Microsoft").join("WinGet").join("Links")),
            Some(home.join(".bun").join("bin")),
            Some(home.join("scoop").join("shims")),
            Some(home.join(".cargo").join("bin")),
            Some(home.join(".local").join("bin")),
        ]
        .into_iter()
        .flatten(),
    );

    let mut seen = std::collections::HashSet::new();
    folders.retain(|f| seen.insert(f.to_string_lossy().to_lowercase()));
    folders
}

fn native_candidates(folders: &[PathBuf]) -> Vec<PathBuf> {
    let (package, triple) = if cfg!(target_arch = "aarch64") {
        ("codex-win32-arm64", "aarch64-pc-windows-msvc")
    } else {
        ("codex-win32-x64", "x86_64-pc-windows-msvc")
    };
    let inside = |root: &Path| root.join("vendor").join(triple).join("codex").join("codex.exe");

    let mut candidates = Vec::new();
    for folder in folders {
        let modules = folder.join("node_modules").join("@openai");
        // The platform package nested under `@openai/codex`, hoisted beside
        // it, or the older layout that kept `vendor` in the package itself.
        candidates.push(inside(&modules.join("codex").join("node_modules").join("@openai").join(package)));
        candidates.push(inside(&modules.join(package)));
        candidates.push(inside(&modules.join("codex")));
        candidates.push(folder.join("codex.exe"));
    }
    candidates
}
