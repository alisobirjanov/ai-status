//! Claude Code's usage.
//!
//! Port of `ClaudeCodeUsageService.swift`. On Windows Claude Code keeps its
//! OAuth login in `%USERPROFILE%\.claude\.credentials.json` (there is no
//! Keychain); an account Pulse added keeps it in a folder of Pulse's own
//! (`paths::claude_account_dir`). Pulse reads a login without ever writing it.
//!
//! The endpoint is not public API — it is what Claude Code itself calls — so
//! it can change without notice. The saved token expires in hours and
//! **Pulse never renews it**: refreshing would rotate the refresh token out
//! from under Claude Code. Once it has expired, Claude Code is asked instead
//! (`claude_code`), and renews it the way it always does.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use super::claude_code::{self, ClaudeCodeError};
use super::http::{self, Outcome};
use crate::model::{number, now_ms, Provider, ProviderUsage, Reason, Route, UsageWindow, WindowKind};
use crate::paths;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";

fn headers(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Authorization", format!("Bearer {token}")),
        ("anthropic-beta", "oauth-2025-04-20".into()),
        ("User-Agent", "claude-cli (external, cli)".into()),
    ]
}

/// The usage of the login in `folder`: an account Pulse added, or with
/// `None` the one Claude Code is signed in to itself.
pub async fn fetch(folder: Option<&Path>) -> ProviderUsage {
    // Read once: an expired token is still evidence of a login, which is the
    // whole difference between "sign in" and "your login expired".
    let Some(credentials) = read_credentials(folder) else {
        return ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeSignInRequired).recording(Route::Endpoint);
    };
    if let Some(token) = unexpired_access_token(&credentials, now_ms()) {
        let owned = headers(&token);
        let borrowed: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
        match http::get_json(USAGE_URL, &borrowed).await {
            Outcome::Ok(root) => {
                let plan = plan_from_credentials(&credentials).or_else(|| ProfilePlan::cached_or_ask(folder, &token));
                return parse(&root, plan).recording(Route::Endpoint);
            }
            // Revoked early, or renewed elsewhere since: Claude Code knows.
            Outcome::NeedsFreshCredentials => {}
            Outcome::Failed(reason) => {
                return ProviderUsage::unavailable(Provider::ClaudeCode, reason).recording(Route::Endpoint);
            }
        }
    }
    from_claude_code(folder).await
}

/// Claude Code, asked for the usage of a login Pulse can't use as it stands.
/// It renews the login as it answers, and the next pass reads it directly.
async fn from_claude_code(folder: Option<&Path>) -> ProviderUsage {
    let answer = match claude_code::usage(folder).await {
        Ok(answer) => answer,
        // Without Claude Code there is nothing to renew it with.
        Err(ClaudeCodeError::NotFound | ClaudeCodeError::StartFailed | ClaudeCodeError::Refused) => {
            return ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeLoginExpired).recording(Route::Endpoint);
        }
        Err(ClaudeCodeError::TimedOut) => {
            return ProviderUsage::unavailable(Provider::ClaudeCode, Reason::Unreachable).recording(Route::ClaudeCode);
        }
    };
    let usage = match answer.get("rate_limits").filter(|limits| limits.is_object()) {
        Some(limits) => {
            let stated = answer.get("subscription_type").and_then(Value::as_str);
            let plan = read_credentials(folder)
                .and_then(|credentials| plan_from_credentials(&credentials))
                .or_else(|| plan_name(stated, None, None));
            parse(limits, plan)
        }
        // No subscription login there that Claude Code can use: signed out
        // since, or one it could not renew.
        None if read_credentials(folder).is_none() => {
            ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeSignInRequired)
        }
        None => ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeLoginExpired),
    };
    usage.recording(Route::ClaudeCode)
}

/// Presence only — never the contents. Marks the chooser row as detected.
pub fn is_installed() -> bool {
    paths::claude_dir().exists()
}

/// Where Claude Code keeps the login of `folder`, or its own.
pub fn credentials_file(folder: Option<&Path>) -> PathBuf {
    folder.map(Path::to_path_buf).unwrap_or_else(paths::claude_dir).join(".credentials.json")
}

fn read_credentials(folder: Option<&Path>) -> Option<Value> {
    let text = std::fs::read_to_string(credentials_file(folder)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Who a login belongs to, as Claude Code noted it when it signed in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Identity {
    /// The same account signed in twice has the same one.
    pub uuid: Option<String>,
    pub email: Option<String>,
}

pub fn identity(folder: Option<&Path>) -> Identity {
    // Without a login there is nobody, whatever was noted before.
    if !credentials_file(folder).is_file() {
        return Identity::default();
    }
    let account = std::fs::read_to_string(paths::claude_global_config(folder))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|root| root.get("oauthAccount").cloned());
    let field = |key: &str| {
        account
            .as_ref()
            .and_then(|a| a.get(key))
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    Identity { uuid: field("accountUuid"), email: field("emailAddress") }
}

/// Whose the login in `folder` is, as the login itself says when asked:
/// `None` when it can't be asked — expired, or no answer.
pub async fn owner(folder: Option<&Path>) -> Option<String> {
    let token = unexpired_access_token(&read_credentials(folder)?, now_ms())?;
    let owned = headers(&token);
    let borrowed: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    match http::get_json(PROFILE_URL, &borrowed).await {
        Outcome::Ok(root) => root.pointer("/account/uuid").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

fn unexpired_access_token(credentials: &Value, now: i64) -> Option<String> {
    let oauth = credentials.get("claudeAiOauth")?;
    let token = oauth.get("accessToken")?.as_str()?;
    // Milliseconds. An expired token counts as absent rather than being spent
    // on a call that can only come back 401.
    if let Some(expires_at) = number(oauth.get("expiresAt")) {
        if expires_at as i64 <= now {
            return None;
        }
    }
    Some(token.to_string())
}

/// Claude Code on Windows writes the plan beside the token, so no second
/// request is needed for it.
fn plan_from_credentials(credentials: &Value) -> Option<String> {
    let oauth = credentials.get("claudeAiOauth")?;
    plan_name(
        oauth.get("subscriptionType").and_then(Value::as_str),
        oauth.get("rateLimitTier").and_then(Value::as_str),
        None,
    )
}

/// The plan from `/api/oauth/profile`, for a login file that does not carry
/// one. Asked rarely and never waited for: the usage reading takes whatever
/// is known now, and a miss is filled in for the next pass. One per login.
struct ProfilePlan;

/// Each login file's plan, and when it was asked.
type Plans = HashMap<PathBuf, (Option<String>, i64)>;

static PROFILE: Mutex<Option<Plans>> = Mutex::new(None);
static ASKING: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
const PROFILE_FRESH_FOR_MS: i64 = 6 * 3600 * 1000;

impl ProfilePlan {
    fn cached_or_ask(folder: Option<&Path>, token: &str) -> Option<String> {
        let key = credentials_file(folder);
        let known = PROFILE.lock().unwrap().get_or_insert_with(HashMap::new).get(&key).cloned();
        let stale = known.as_ref().map_or(true, |(_, at)| now_ms() - at >= PROFILE_FRESH_FOR_MS);
        if stale {
            let mut asking = ASKING.lock().unwrap();
            if asking.get_or_insert_with(HashSet::new).insert(key.clone()) {
                let token = token.to_string();
                tauri::async_runtime::spawn(async move {
                    let owned = headers(&token);
                    let borrowed: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
                    let name = match http::get_json(PROFILE_URL, &borrowed).await {
                        Outcome::Ok(root) => {
                            let organization = root.get("organization");
                            let field = |key: &str| {
                                organization
                                    .and_then(|o| o.get(key))
                                    .or_else(|| root.get(key))
                                    .and_then(Value::as_str)
                            };
                            plan_name(field("subscription_type"), field("rate_limit_tier"), field("organization_type"))
                        }
                        _ => None,
                    };
                    // A failure is remembered too: the plan is a nicety and
                    // must never cost the reading a retry every pass.
                    PROFILE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key.clone(), (name, now_ms()));
                    ASKING.lock().unwrap().get_or_insert_with(HashSet::new).remove(&key);
                });
            }
        }
        known.and_then(|(name, _)| name)
    }
}

/// The name on the plan. `subscription_type` first; the multiplier only ever
/// lives on the tier (`default_claude_max_20x` → "Max 20x"), so it is kept
/// whichever name wins. Anything unfamiliar is tidied and passed through.
pub fn plan_name(subscription: Option<&str>, tier: Option<&str>, organization_type: Option<&str>) -> Option<String> {
    let tier = tier.map(str::to_lowercase);
    let multiplier = tier
        .as_deref()
        .and_then(|t| ["20x", "5x"].into_iter().find(|m| t.ends_with(&format!("_{m}"))));
    let with_multiplier = |name: String| match multiplier {
        Some(m) => format!("{name} {m}"),
        None => name,
    };

    if let Some(stated) = subscription.filter(|s| !s.is_empty()) {
        return Some(with_multiplier(tidy(stated)));
    }
    let Some(tier) = tier else {
        return organization_type.map(tidy);
    };
    let base = if tier.contains("max") {
        "Max".to_string()
    } else if tier.contains("team") {
        "Team".to_string()
    } else if tier.contains("enterprise") {
        "Enterprise".to_string()
    } else if tier.contains("pro") {
        "Pro".to_string()
    } else if tier.contains("free") {
        "Free".to_string()
    } else {
        tidy(&tier)
    };
    Some(with_multiplier(base))
}

/// `claude_max` reads as a field name; "Claude Max" reads as a plan.
fn tidy(raw: &str) -> String {
    raw.split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// MARK: - Parsing

/// `limits[]` is the fuller answer — it carries per-model weekly windows,
/// which the top-level `five_hour` / `seven_day` pair does not.
pub fn parse(root: &Value, plan: Option<String>) -> ProviderUsage {
    let mut windows: Vec<UsageWindow> = root
        .get("limits")
        .and_then(Value::as_array)
        .map(|limits| limits.iter().filter_map(window_from_limit).collect())
        .unwrap_or_default();

    if windows.is_empty() {
        for (key, kind, seconds) in [("five_hour", WindowKind::FiveHour, 5 * 3600), ("seven_day", WindowKind::Weekly, 7 * 86_400)] {
            let Some(node) = root.get(key) else { continue };
            let Some(percent) = number(node.get("utilization")) else { continue };
            windows.push(UsageWindow {
                id: format!("claudeCode.{key}"),
                kind,
                scope: None,
                used_fraction: percent / 100.0,
                window_seconds: seconds,
                resets_at: node.get("resets_at").and_then(Value::as_str).and_then(parse_date),
                is_exhausted: is_spent(node),
            });
        }
    }

    ProviderUsage::live(Provider::ClaudeCode, windows, plan, None)
}

fn window_from_limit(limit: &Value) -> Option<UsageWindow> {
    let kind_name = limit.get("kind")?.as_str()?;
    let percent = number(limit.get("percent"))?;
    let (kind, seconds) = match kind_name {
        "session" => (WindowKind::FiveHour, 5 * 3600),
        "weekly_all" | "weekly_scoped" => (WindowKind::Weekly, 7 * 86_400),
        _ => return None,
    };
    // A scoped limit names the model it applies to; an unscoped one covers
    // the whole account.
    let scope = limit
        .get("scope")
        .and_then(|s| s.get("model"))
        .and_then(|m| m.get("display_name"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(UsageWindow {
        id: format!("claudeCode.{kind_name}.{}", scope.as_deref().unwrap_or("all")),
        kind,
        scope,
        used_fraction: percent / 100.0,
        window_seconds: seconds,
        resets_at: limit.get("resets_at").and_then(Value::as_str).and_then(parse_date),
        is_exhausted: is_spent(limit),
    })
}

/// Whether Claude Code says this limit is spent. `locked_reason` is
/// unambiguous. A **warning is not a block** — Claude Code raises `severity`
/// to `warning` while a limit still has room (seen at 76%). Anything else it
/// has not been seen to say errs towards "you're blocked".
fn is_spent(limit: &Value) -> bool {
    if limit.get("locked_reason").is_some_and(|v| !v.is_null()) {
        return true;
    }
    let Some(severity) = limit.get("severity").and_then(Value::as_str) else {
        return false;
    };
    !["normal", "ok", "none", "healthy", "warning", "warn"].contains(&severity.to_lowercase().as_str())
}

fn parse_date(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn limits_array_wins_and_carries_model_scope() {
        let root = json!({
            "five_hour": { "utilization": 99, "resets_at": "2030-01-01T00:00:00Z" },
            "limits": [
                { "kind": "session", "percent": 12, "severity": "normal", "resets_at": "2030-01-01T05:00:00.123+00:00" },
                { "kind": "weekly_all", "percent": 40, "severity": "ok" },
                { "kind": "weekly_scoped", "percent": 76, "severity": "warning",
                  "scope": { "model": { "display_name": "Opus" } } },
                { "kind": "something_new", "percent": 5 }
            ]
        });
        let usage = parse(&root, Some("Max 20x".into()));
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].kind, WindowKind::FiveHour);
        assert!((usage.windows[0].used_fraction - 0.12).abs() < 1e-9);
        assert_eq!(usage.windows[0].resets_at, Some(1_893_474_000_123));
        assert_eq!(usage.windows[2].scope.as_deref(), Some("Opus"));
        assert_eq!(usage.windows[2].id, "claudeCode.weekly_scoped.Opus");
        // A warning is not a block.
        assert!(!usage.windows[2].is_exhausted);
        assert_eq!(usage.plan.as_deref(), Some("Max 20x"));
    }

    #[test]
    fn falls_back_to_top_level_pair() {
        let root = json!({
            "five_hour": { "utilization": 30.5, "resets_at": "2030-01-01T00:00:00Z" },
            "seven_day": { "utilization": 10 }
        });
        let usage = parse(&root, None);
        assert_eq!(usage.windows.len(), 2);
        assert_eq!(usage.windows[1].kind, WindowKind::Weekly);
    }

    #[test]
    fn spent_is_the_providers_word() {
        assert!(is_spent(&json!({ "locked_reason": "limit" })));
        assert!(is_spent(&json!({ "severity": "blocked" })));
        assert!(!is_spent(&json!({ "severity": "WARNING" })));
        assert!(!is_spent(&json!({ "locked_reason": null })));
        assert!(!is_spent(&json!({ "percent": 100 })));
    }

    #[test]
    fn nothing_reported_is_said() {
        let usage = parse(&json!({ "limits": [] }), None);
        assert_eq!(usage.reason, Some(Reason::NoLimitsReported));
    }

    #[test]
    fn plan_names() {
        assert_eq!(plan_name(Some("max"), Some("default_claude_max_20x"), None).as_deref(), Some("Max 20x"));
        assert_eq!(plan_name(Some("pro"), None, None).as_deref(), Some("Pro"));
        assert_eq!(plan_name(None, Some("default_claude_max_5x"), None).as_deref(), Some("Max 5x"));
        assert_eq!(plan_name(Some("claude_team"), None, None).as_deref(), Some("Claude Team"));
        assert_eq!(plan_name(None, None, Some("claude_enterprise")).as_deref(), Some("Claude Enterprise"));
        assert_eq!(plan_name(None, None, None), None);
    }

    #[test]
    fn expired_token_is_absent() {
        let credentials = json!({ "claudeAiOauth": { "accessToken": "t", "expiresAt": 1000 } });
        assert_eq!(unexpired_access_token(&credentials, 999).as_deref(), Some("t"));
        assert_eq!(unexpired_access_token(&credentials, 1000), None);
    }
}
