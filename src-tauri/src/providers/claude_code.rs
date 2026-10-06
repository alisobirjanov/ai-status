//! Claude Code itself, run as any session of it would be, for what Dipstick
//! can't do without holding a Claude login of its own: renewing one that has
//! expired, and signing in to begin with.
//!
//! Nothing here touches a token. Claude Code reads its login from the folder
//! it is pointed at (`CLAUDE_CONFIG_DIR`, or its own) and renews it there;
//! Dipstick only asks it for the usage `/usage` shows, over its SDK mode's
//! control protocol. There is no prompt, so no model is called and nothing
//! is spent; no transcript is kept; hooks are off. Asked again within a
//! minute, Claude Code answers from what it fetched last. The protocol is
//! marked experimental, so whatever it says is read defensively.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

use super::app_server;
use crate::paths;

#[derive(Debug)]
pub enum ClaudeCodeError {
    /// Claude Code isn't installed, or isn't anywhere we thought to look.
    NotFound,
    StartFailed,
    TimedOut,
    /// It answered, and the answer was no.
    Refused,
}

/// Starting is the slow part: a second or two, more from a cold disk.
const USAGE_TIMEOUT: Duration = Duration::from_secs(45);
/// Answered, it is given this long to finish what it was writing and go
/// before it is taken down.
const EXIT_GRACE: Duration = Duration::from_secs(5);
/// How long a sign-in in the browser is waited for.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const SIGN_OUT_TIMEOUT: Duration = Duration::from_secs(30);

/// What Claude Code's usage is, as it would show it in `/usage`.
pub async fn usage(folder: Option<&Path>) -> Result<Value, ClaudeCodeError> {
    let executable = locate().ok_or(ClaudeCodeError::NotFound)?;
    let mut command = command(&executable, folder);
    command
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
        // Nothing of the session is kept, and nothing of the reader's runs:
        // no MCP servers, no hooks.
        .args(["--no-session-persistence", "--strict-mcp-config", "--settings"])
        .arg(quiet_settings()?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().map_err(|_| ClaudeCodeError::StartFailed)?;
    let pid = child.id();
    let mut stdin = child.stdin.take().ok_or(ClaudeCodeError::StartFailed)?;
    let stdout = child.stdout.take().ok_or(ClaudeCodeError::StartFailed)?;

    let conversation = async {
        let mut lines = BufReader::new(stdout).lines();
        let request = |id: &str, subtype: &str| {
            let mut body = json!({ "subtype": subtype });
            if subtype == "get_usage" {
                body["skip_behaviors"] = json!(true);
            }
            json!({ "type": "control_request", "request_id": id, "request": body })
        };
        send(&mut stdin, request("init", "initialize")).await?;
        answer(&mut lines, "init").await?;
        send(&mut stdin, request("usage", "get_usage")).await?;
        answer(&mut lines, "usage").await
    };
    let result = match tokio::time::timeout(USAGE_TIMEOUT, conversation).await {
        Ok(result) => result,
        Err(_) => Err(ClaudeCodeError::TimedOut),
    };

    // No more input ends the session. A login it renewed on the way is
    // already saved — the usage could not have been asked without it — but
    // it is let go on its own before anything is cut short.
    drop(stdin);
    if tokio::time::timeout(EXIT_GRACE, child.wait()).await.is_err() {
        app_server::shut_down(&mut child, pid).await;
    }
    result
}

#[derive(Debug)]
pub enum SignInError {
    NotFound,
    StartFailed,
    TimedOut,
    Cancelled,
    /// Claude Code's own words, when it gave any.
    Failed(Option<String>),
}

/// Sign in to a Claude account in the browser, the login kept in `folder`.
/// Claude Code opens the browser itself and waits for the sign-in there;
/// this waits for Claude Code, until `cancelled` says otherwise.
pub async fn sign_in(folder: Option<&Path>, cancelled: &tokio::sync::Notify) -> Result<(), SignInError> {
    let executable = locate().ok_or(SignInError::NotFound)?;
    let mut command = command(&executable, folder);
    command
        .args(["auth", "login", "--claudeai"])
        // Held open and never written to: it also reads a code typed in by
        // hand, for when the browser can't hand it back.
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|_| SignInError::StartFailed)?;
    let pid = child.id();
    let _stdin = child.stdin.take();
    let stderr = child.stderr.take();

    let outcome = tokio::select! {
        status = child.wait() => match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(SignInError::Failed(match stderr {
                Some(stderr) => failure(stderr).await,
                None => None,
            })),
            Err(_) => Err(SignInError::StartFailed),
        },
        _ = cancelled.notified() => Err(SignInError::Cancelled),
        _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => Err(SignInError::TimedOut),
    };
    if outcome.is_err() {
        app_server::shut_down(&mut child, pid).await;
    }
    outcome
}

/// "Login failed: …" as Claude Code put it, a line of it.
async fn failure(mut stderr: tokio::process::ChildStderr) -> Option<String> {
    let mut text = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), stderr.read_to_string(&mut text)).await;
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("Login failed:"))
        .map(|reason| reason.trim().chars().take(200).collect::<String>())
        .filter(|reason| !reason.is_empty())
}

/// Sign the login in `folder` out, as Claude Code does it.
pub async fn sign_out(folder: &Path) {
    let Some(executable) = locate() else { return };
    let mut command = command(&executable, Some(folder));
    command.args(["auth", "logout"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else { return };
    let pid = child.id();
    if tokio::time::timeout(SIGN_OUT_TIMEOUT, child.wait()).await.is_err() {
        app_server::shut_down(&mut child, pid).await;
    }
}

fn command(executable: &Path, folder: Option<&Path>) -> Command {
    let mut command = Command::new(executable);
    command.kill_on_drop(true);
    // Claude Code's own login is wherever Claude Code finds it, which
    // `CLAUDE_CONFIG_DIR` inherited from the reader's environment may move:
    // set to its usual place instead, it would read a different settings file.
    if let Some(folder) = folder {
        command.env("CLAUDE_CONFIG_DIR", folder);
    }
    // A login of the environment's would stand in for the one in the folder.
    for name in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_OAUTH_REFRESH_TOKEN"] {
        command.env_remove(name);
    }
    // Nothing but what was asked: no telemetry or error reports, and no
    // update installed because Dipstick happened to start it.
    command.env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1").env("DISABLE_AUTOUPDATER", "1");
    // Somewhere no project is, so none of one's settings apply.
    let place = paths::data_dir();
    let _ = std::fs::create_dir_all(&place);
    command.current_dir(place);
    // The folder it was found in leads PATH: an npm install is a Node
    // script, and `node` lives beside it.
    if let Some(folder) = executable.parent() {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let mut leading = std::ffi::OsString::from(folder.as_os_str());
        leading.push(";");
        leading.push(inherited);
        command.env("PATH", leading);
    }
    #[cfg(windows)]
    command.creation_flags(app_server::CREATE_NO_WINDOW);
    command
}

/// Hooks off, in a file rather than on the command line: an npm install is
/// started through `cmd`, which has its own idea of quotes.
fn quiet_settings() -> Result<PathBuf, ClaudeCodeError> {
    let file = paths::data_dir().join("claude-code-settings.json");
    let wanted = br#"{"disableAllHooks":true}"#;
    if std::fs::read(&file).ok().as_deref() != Some(&wanted[..]) {
        paths::write_atomically(&file, wanted).map_err(|_| ClaudeCodeError::StartFailed)?;
    }
    Ok(file)
}

async fn send(stdin: &mut ChildStdin, message: Value) -> Result<(), ClaudeCodeError> {
    let mut line = serde_json::to_vec(&message).map_err(|_| ClaudeCodeError::StartFailed)?;
    line.push(b'\n');
    stdin.write_all(&line).await.map_err(|_| ClaudeCodeError::StartFailed)?;
    stdin.flush().await.map_err(|_| ClaudeCodeError::StartFailed)
}

/// Newline-delimited JSON. Anything but the answer waited for — the session
/// starting, a line that is not JSON — is passed over. EOF means it has gone.
async fn answer(lines: &mut tokio::io::Lines<BufReader<ChildStdout>>, id: &str) -> Result<Value, ClaudeCodeError> {
    loop {
        let line = lines
            .next_line()
            .await
            .map_err(|_| ClaudeCodeError::StartFailed)?
            .ok_or(ClaudeCodeError::StartFailed)?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message.get("type").and_then(Value::as_str) != Some("control_response") {
            continue;
        }
        let Some(response) = message.get("response") else { continue };
        if response.get("request_id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        if response.get("subtype").and_then(Value::as_str) != Some("success") {
            return Err(ClaudeCodeError::Refused);
        }
        return Ok(response.get("response").cloned().unwrap_or_else(|| json!({})));
    }
}

// MARK: - Finding claude

/// Whether there is a Claude Code to sign in with and renew logins.
pub fn is_available() -> bool {
    locate().is_some()
}

/// The native `claude.exe` where there is one, an npm shim otherwise, and
/// failing both the copy an editor's extension carries. Looked for at most
/// once a minute: Settings asks with every change it draws.
fn locate() -> Option<PathBuf> {
    static FOUND: Mutex<Option<(Option<PathBuf>, Instant)>> = Mutex::new(None);
    let mut found = FOUND.lock().unwrap();
    if let Some((path, at)) = found.as_ref() {
        let still_there = path.as_ref().map_or(true, |p| p.is_file());
        if at.elapsed() < Duration::from_secs(60) && still_there {
            return path.clone();
        }
    }
    let folders = app_server::search_folders();
    let path = folders
        .iter()
        .map(|f| f.join("claude.exe"))
        .chain(folders.iter().map(|f| f.join("claude.cmd")))
        .chain(editor_copies(&paths::home()))
        .find(|candidate| candidate.is_file());
    *found = Some((path.clone(), Instant::now()));
    path
}

/// The copies VS Code's extension and its forks' carry, newest first, for
/// somebody who only ever uses Claude Code in an editor.
fn editor_copies(home: &Path) -> Vec<PathBuf> {
    let mut copies: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for editor in [".vscode", ".vscode-insiders", ".cursor", ".windsurf"] {
        let Ok(entries) = std::fs::read_dir(home.join(editor).join("extensions")) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix("anthropic.claude-code-") else { continue };
            copies.push((version_of(rest), entry.path().join("resources").join("native-binary").join("claude.exe")));
        }
    }
    copies.sort_by(|a, b| b.0.cmp(&a.0));
    copies.into_iter().map(|(_, path)| path).collect()
}

/// `2.1.291-win32-x64` → `[2, 1, 291]`.
fn version_of(text: &str) -> Vec<u32> {
    text.split(['.', '-']).map_while(|part| part.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_editor_copy_wins() {
        assert_eq!(version_of("2.1.291-win32-x64"), vec![2, 1, 291]);
        assert!(version_of("2.1.291-win32-x64") > version_of("2.1.29-win32-x64"));
        assert!(version_of("2.10.0") > version_of("2.9.999"));
    }
}
