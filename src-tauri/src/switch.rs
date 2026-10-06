//! Which account Claude Code itself is signed in to, changed without signing
//! in again: an account Dipstick holds hands Claude Code its login.
//!
//! Claude Code keeps a login in two files: the tokens in `.credentials.json`,
//! whose they are in `.claude.json`. Signing in to another account with
//! `/login` changes only part of each — in the first, what it drops when it
//! signs out; in the second, the account and what it had fetched for it — and
//! so does this. Everything else, the reader's projects and settings and MCP
//! servers' logins, is left as it was, byte for byte.
//!
//! A login is only ever used from one place. Before Claude Code is handed
//! another, the one it has goes back to the folder of the account it is —
//! renewed since, it is newer than what that folder had — or into a new one,
//! so a login is never simply overwritten. While an account is in Claude
//! Code, its own folder is left alone (`Store::mirrors`).

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::value::RawValue;

/// What Claude Code drops from its credentials when it signs out. MCP
/// servers' logins are the PC's rather than the account's, and stay.
const LOGIN_KEYS: &[&str] = &["claudeAiOauth", "organizationUuid", "trustedDeviceToken", "enterpriseGateway", "designOauth"];

/// What Claude Code forgets of an account in `.claude.json` when it signs
/// out, besides who it was: what it had fetched for it, and fetches again for
/// the next. As Claude Code 2.1 has it.
const ACCOUNT_CACHES: &[&str] = &[
    "additionalModelOptionsCache",
    "additionalModelOptionsAnsweredAt",
    "additionalModelCostsCache",
    "modelAccessCache",
    "orgModelDefaultCache",
    "cachedArtifactRoster",
    "artifactRosterDenied",
    "lastSeenOrgDefaultUpdatedAt",
    "clientDataCache",
    "clientDataCacheSlots",
    "autoCompactWindowsCache",
    "cachedUsageUtilization",
    "metricsStatusCache",
    "metricsStatusCacheByPrincipal",
    "githubWebConnectionStatusCache",
    "startupPrefetchedAt",
];

/// Where a login is kept: Claude Code's own place, or an account's folder.
#[derive(Clone, Debug)]
pub struct Place {
    pub credentials: PathBuf,
    pub config: PathBuf,
}

impl Place {
    /// Wherever Claude Code looks, `CLAUDE_CONFIG_DIR` included.
    pub fn claude_code() -> Place {
        Place {
            credentials: crate::providers::claude::credentials_file(None),
            config: crate::paths::claude_global_config(None),
        }
    }

    pub fn folder(folder: &Path) -> Place {
        Place { credentials: folder.join(".credentials.json"), config: folder.join(".claude.json") }
    }
}

#[derive(Debug, PartialEq)]
pub enum Problem {
    /// The account to hand over has no login to hand.
    NoLogin,
    /// Claude Code has a login and nowhere was given for it to go back to.
    NowhereBack,
    /// A file couldn't be read as Claude Code writes it, or couldn't be written.
    Failed,
}

/// Hand `account`'s login to Claude Code. The login Claude Code has goes to
/// `back` first; if that can't be done, Claude Code's is left as it was.
pub fn hand_over(claude_code: &Place, account: &Place, back: Option<&Place>) -> Result<(), Problem> {
    let given = Login::read(account)?;
    if given.credentials.get("claudeAiOauth").is_none() || given.config.get("oauthAccount").is_none() {
        return Err(Problem::NoLogin);
    }
    let mut own = Login::read(claude_code)?;
    if own.credentials.get("claudeAiOauth").is_some() {
        let back = back.ok_or(Problem::NowhereBack)?;
        let mut kept = Login::read(back)?;
        kept.take(&own, false);
        kept.write(back)?;
    }
    own.take(&given, true);
    own.write(claude_code)
}

/// A login's two files, as read.
struct Login {
    credentials: Document,
    config: Document,
}

impl Login {
    fn read(place: &Place) -> Result<Login, Problem> {
        Ok(Login {
            credentials: Document::read(&place.credentials, None)?,
            config: Document::read(&place.config, Some("  "))?,
        })
    }

    /// `other`'s login in place of this one. `fresh` also forgets what was
    /// fetched for the account this one was.
    fn take(&mut self, other: &Login, fresh: bool) {
        for key in LOGIN_KEYS {
            match other.credentials.get(key) {
                Some(value) => self.credentials.set(key, value),
                None => self.credentials.remove(key),
            }
        }
        if let Some(account) = other.config.get("oauthAccount") {
            self.config.set("oauthAccount", account);
        }
        if fresh {
            for key in ACCOUNT_CACHES {
                self.config.remove(key);
            }
        }
    }

    /// The tokens, then whose they are. A failure halfway puts the tokens
    /// back as they were.
    fn write(&self, place: &Place) -> Result<(), Problem> {
        let credentials = self.credentials.text()?;
        let config = self.config.text()?;
        self.credentials.replace(&place.credentials, &credentials)?;
        if let Err(problem) = self.config.replace(&place.config, &config) {
            self.credentials.restore(&place.credentials);
            return Err(problem);
        }
        Ok(())
    }
}

/// A JSON object as Claude Code wrote it: its entries in their order, each
/// value exactly as written, and how it was laid out.
struct Document {
    entries: Vec<(String, String)>,
    /// One level of indentation; `None`, all on one line.
    indent: Option<String>,
    newline: &'static str,
    /// Whatever followed the closing brace.
    tail: String,
    /// The file as it was; `None`, there was none.
    original: Option<Vec<u8>>,
}

impl Document {
    /// A file that isn't there is an empty object, laid out with `indent`.
    fn read(path: &Path, indent: Option<&str>) -> Result<Document, Problem> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| Problem::Failed)?;
                let mut document = Document::parse(text).ok_or(Problem::Failed)?;
                document.original = Some(bytes);
                Ok(document)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Document {
                entries: Vec::new(),
                indent: indent.map(str::to_string),
                newline: "\n",
                tail: String::new(),
                original: None,
            }),
            Err(_) => Err(Problem::Failed),
        }
    }

    fn parse(text: &str) -> Option<Document> {
        let Entries(entries) = serde_json::from_str(text).ok()?;
        let end = text.rfind('}')?;
        let indent = text.find('\n').map(|at| {
            let indent: String = text[at + 1..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            if indent.is_empty() { "  ".to_string() } else { indent }
        });
        Some(Document {
            entries,
            indent,
            newline: if text.contains("\r\n") { "\r\n" } else { "\n" },
            tail: text[end + 1..].to_string(),
            original: None,
        })
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, value)| value.as_str())
    }

    fn set(&mut self, key: &str, value: &str) {
        let value = laid_out(value, self.indent.as_deref(), self.newline);
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => *old = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }

    fn remove(&mut self, key: &str) {
        self.entries.retain(|(k, _)| k != key);
    }

    fn text(&self) -> Result<String, Problem> {
        let mut out = String::from("{");
        for (at, (key, value)) in self.entries.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            if let Some(indent) = &self.indent {
                out.push_str(self.newline);
                out.push_str(indent);
            }
            out.push_str(&serde_json::to_string(key).map_err(|_| Problem::Failed)?);
            out.push_str(if self.indent.is_some() { ": " } else { ":" });
            out.push_str(value);
        }
        if self.indent.is_some() && !self.entries.is_empty() {
            out.push_str(self.newline);
        }
        out.push('}');
        out.push_str(&self.tail);
        // Never anything Claude Code couldn't read back.
        let check: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&out).map_err(|_| Problem::Failed)?;
        if check.len() != self.entries.len() {
            return Err(Problem::Failed);
        }
        Ok(out)
    }

    /// Write the file whole or not at all, waiting out a moment another
    /// program has it open. Unchanged, it isn't written.
    fn replace(&self, path: &Path, text: &str) -> Result<(), Problem> {
        if self.original.as_deref() == Some(text.as_bytes()) {
            return Ok(());
        }
        write_whole(path, text.as_bytes())
    }

    fn restore(&self, path: &Path) {
        match &self.original {
            Some(bytes) => {
                let _ = write_whole(path, bytes);
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), Problem> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| Problem::Failed)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".dipstick");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).map_err(|_| Problem::Failed)?;
    for attempt in 0..20 {
        if std::fs::rename(&temporary, path).is_ok() {
            return Ok(());
        }
        if attempt < 19 {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = std::fs::remove_file(&temporary);
    Err(Problem::Failed)
}

/// A value laid out the way `JSON.stringify` lays it out one level in: all on
/// one line without an indent, one entry to a line with one.
fn laid_out(value: &str, indent: Option<&str>, newline: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut depth = 1;
    let (mut in_string, mut escaped) = (false, false);
    let line = |out: &mut String, depth: usize| {
        if let Some(indent) = indent {
            out.push_str(newline);
            for _ in 0..depth {
                out.push_str(indent);
            }
        }
    };
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => {
                    in_string = true;
                    out.push(c);
                }
                '{' | '[' => {
                    out.push(c);
                    let close = if c == '{' { '}' } else { ']' };
                    let next = (at + 1..chars.len()).find(|&i| !chars[i].is_whitespace());
                    // `{}` and `[]` stay on their line.
                    match next {
                        Some(i) if chars[i] == close => {
                            out.push(close);
                            at = i;
                        }
                        _ => {
                            depth += 1;
                            line(&mut out, depth);
                        }
                    }
                }
                '}' | ']' => {
                    depth -= 1;
                    line(&mut out, depth);
                    out.push(c);
                }
                ',' => {
                    out.push(c);
                    line(&mut out, depth);
                }
                ':' => out.push_str(if indent.is_some() { ": " } else { ":" }),
                c if c.is_whitespace() => {}
                c => out.push(c),
            }
        }
        at += 1;
    }
    out
}

/// An object's entries in the order they were written, each value as written.
struct Entries(Vec<(String, String)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Walk;
        impl<'de> Visitor<'de> for Walk {
            type Value = Entries;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries, A::Error> {
                let mut entries = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    entries.push((key, value.get().to_string()));
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(Walk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("dipstick-switch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn folder(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// As Claude Code writes them: credentials on one line, settings with
    /// `JSON.stringify(_, null, 2)`.
    fn write(path: &Path, value: &Value, pretty: bool) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = if pretty { serde_json::to_string_pretty(value).unwrap() } else { value.to_string() };
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn keys(path: &Path) -> Vec<String> {
        let Entries(entries) = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        entries.into_iter().map(|(key, _)| key).collect()
    }

    fn login(place: &Place, who: &str, token: &str) {
        write(&place.credentials, &json!({ "claudeAiOauth": { "accessToken": token, "refreshToken": format!("r-{token}") } }), false);
        write(&place.config, &json!({ "oauthAccount": { "accountUuid": who, "emailAddress": format!("{who}@example.com") } }), true);
    }

    #[test]
    fn a_file_is_written_back_as_it_was_read() {
        let pretty = "{\n  \"zebra\": 1,\n  \"apple\": {\n    \"b\": [],\n    \"a\": [\n      1,\n      \"x, y: {z}\"\n    ]\n  },\n  \"empty\": {}\n}";
        let flat = r#"{"zebra":1,"apple":{"b":[],"a":[1,"x, y: {z}"]},"empty":{}}"#;
        let crlf = pretty.replace('\n', "\r\n") + "\r\n";
        for text in [pretty, flat, crlf.as_str()] {
            assert_eq!(Document::parse(text).unwrap().text().unwrap(), text);
        }
    }

    #[test]
    fn a_value_takes_the_layout_of_where_it_goes() {
        let flat = r#"{"a":[1,{"b":"c, d"}],"e":{}}"#;
        let pretty = "{\n    \"a\": [\n      1,\n      {\n        \"b\": \"c, d\"\n      }\n    ],\n    \"e\": {}\n  }";
        assert_eq!(laid_out(flat, Some("  "), "\n"), pretty);
        assert_eq!(laid_out(pretty, None, "\n"), flat);
        assert_eq!(laid_out(r#""a \" { b""#, Some("  "), "\n"), r#""a \" { b""#);
    }

    #[test]
    fn claude_code_is_handed_a_login_and_its_own_goes_back() {
        let scratch = Scratch::new("hand");
        let own = Place { credentials: scratch.folder("home/.claude/.credentials.json"), config: scratch.folder("home/.claude.json") };
        let work = Place::folder(&scratch.folder("work"));
        let personal = Place::folder(&scratch.folder("personal"));
        write(
            &own.credentials,
            &json!({ "claudeAiOauth": { "accessToken": "p2", "refreshToken": "r-p2" }, "mcpOAuth": { "server": 1 }, "organizationUuid": "org-p" }),
            false,
        );
        write(
            &own.config,
            &json!({ "numStartups": 3, "oauthAccount": { "accountUuid": "p", "emailAddress": "p@example.com" }, "projects": { "E:/x": {} }, "modelAccessCache": { "p": 1 }, "userID": "u" }),
            true,
        );
        login(&work, "w", "w1");
        // What the personal account's folder had is older than what Claude Code has now.
        login(&personal, "p", "p1");
        let order = keys(&own.config);

        hand_over(&own, &work, Some(&personal)).unwrap();

        let credentials = read(&own.credentials);
        assert_eq!(credentials["claudeAiOauth"]["accessToken"], "w1");
        assert_eq!(credentials["mcpOAuth"], json!({ "server": 1 }));
        // The personal account's organisation isn't the work one's.
        assert!(credentials.get("organizationUuid").is_none());
        let config = read(&own.config);
        assert_eq!(config["oauthAccount"]["accountUuid"], "w");
        assert!(config.get("modelAccessCache").is_none());
        assert_eq!(config["projects"], json!({ "E:/x": {} }));
        assert_eq!(keys(&own.config), order.into_iter().filter(|k| k != "modelAccessCache").collect::<Vec<_>>());

        // The login Claude Code had is the personal account's again, as renewed.
        let kept = read(&personal.credentials);
        assert_eq!(kept["claudeAiOauth"]["accessToken"], "p2");
        assert_eq!(kept["organizationUuid"], "org-p");
        assert!(kept.get("mcpOAuth").is_none());
        assert_eq!(read(&personal.config)["oauthAccount"]["accountUuid"], "p");
        // The work account's folder is as it was.
        assert_eq!(read(&work.credentials)["claudeAiOauth"]["accessToken"], "w1");
    }

    #[test]
    fn a_login_with_nowhere_to_go_is_kept_in_a_new_folder() {
        let scratch = Scratch::new("new");
        let own = Place::folder(&scratch.folder("own"));
        let work = Place::folder(&scratch.folder("work"));
        let new = Place::folder(&scratch.folder("new"));
        login(&own, "p", "p1");
        login(&work, "w", "w1");

        hand_over(&own, &work, Some(&new)).unwrap();

        assert_eq!(read(&new.credentials)["claudeAiOauth"]["accessToken"], "p1");
        assert_eq!(read(&new.config)["oauthAccount"]["accountUuid"], "p");
        assert!(!std::fs::read_to_string(&new.credentials).unwrap().contains('\n'));
        assert!(std::fs::read_to_string(&new.config).unwrap().starts_with("{\n  \"oauthAccount\": {\n    \""));
        assert_eq!(read(&own.credentials)["claudeAiOauth"]["accessToken"], "w1");
    }

    #[test]
    fn nothing_changes_when_it_cannot_be_done() {
        let scratch = Scratch::new("refused");
        let own = Place::folder(&scratch.folder("own"));
        let work = Place::folder(&scratch.folder("work"));
        let gone = Place::folder(&scratch.folder("gone"));
        login(&own, "p", "p1");
        login(&work, "w", "w1");
        let before = (std::fs::read(&own.credentials).unwrap(), std::fs::read(&own.config).unwrap());

        assert_eq!(hand_over(&own, &gone, None), Err(Problem::NoLogin));
        assert_eq!(hand_over(&own, &work, None), Err(Problem::NowhereBack));
        assert_eq!((std::fs::read(&own.credentials).unwrap(), std::fs::read(&own.config).unwrap()), before);

        // Signed out, Claude Code has nothing to give back.
        write(&own.credentials, &json!({ "mcpOAuth": {} }), false);
        hand_over(&own, &work, None).unwrap();
        assert_eq!(read(&own.credentials)["claudeAiOauth"]["accessToken"], "w1");
        assert_eq!(read(&own.credentials)["mcpOAuth"], json!({}));
    }
}
