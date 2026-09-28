//! Claude Code's usage.
//!
//! Port of `ClaudeCodeUsageService.swift`, endpoint route only. On Windows
//! Claude Code keeps its OAuth login in `%USERPROFILE%\.claude\.credentials.json`
//! (there is no Keychain), and Pulse reads it without ever writing it.
//!
//! The endpoint is not public API — it is what Claude Code itself calls — so
//! it can change without notice. The saved token expires in hours and
//! **nothing here renews it**: refreshing would rotate the refresh token out
//! from under Claude Code. An expired one is reported as such.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

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

pub async fn fetch() -> ProviderUsage {
    // Read once: an expired token is still evidence of a login, which is the
    // whole difference between "sign in" and "your login expired".
    let credentials = read_credentials();
    let Some(credentials) = credentials else {
        return ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeSignInRequired).recording(Route::Endpoint);
    };
    let Some(token) = unexpired_access_token(&credentials, now_ms()) else {
        return ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeLoginExpired).recording(Route::Endpoint);
    };

    let owned = headers(&token);
    let borrowed: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let usage = match http::get_json(USAGE_URL, &borrowed).await {
        Outcome::Ok(root) => {
            let plan = plan_from_credentials(&credentials).or_else(|| ProfilePlan::cached_or_ask(&token));
            parse(&root, plan)
        }
        Outcome::NeedsFreshCredentials => ProviderUsage::unavailable(Provider::ClaudeCode, Reason::ClaudeLoginExpired),
        Outcome::Failed(reason) => ProviderUsage::unavailable(Provider::ClaudeCode, reason),
    };
    usage.recording(Route::Endpoint)
}

/// Presence only — never the contents. Marks the chooser row as detected.
pub fn is_installed() -> bool {
    paths::claude_dir().exists()
}

/// Where Claude Code keeps its login.
pub fn credentials_file() -> PathBuf {
    paths::claude_dir().join(".credentials.json")
}

fn read_credentials() -> Option<Value> {
    let text = std::fs::read_to_string(credentials_file()).ok()?;
    serde_json::from_str(&text).ok()
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
/// is known now, and a miss is filled in for the next pass.
struct ProfilePlan;

static PROFILE: Mutex<Option<(Option<String>, i64)>> = Mutex::new(None);
static ASKING: Mutex<bool> = Mutex::new(false);
const PROFILE_FRESH_FOR_MS: i64 = 6 * 3600 * 1000;

impl ProfilePlan {
    fn cached_or_ask(token: &str) -> Option<String> {
        let known = PROFILE.lock().unwrap().clone();
        let stale = known.as_ref().map_or(true, |(_, at)| now_ms() - at >= PROFILE_FRESH_FOR_MS);
        if stale {
            let mut asking = ASKING.lock().unwrap();
            if !*asking {
                *asking = true;
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
                    *PROFILE.lock().unwrap() = Some((name, now_ms()));
                    *ASKING.lock().unwrap() = false;
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
