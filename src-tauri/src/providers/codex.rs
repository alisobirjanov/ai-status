//! Codex's usage.
//!
//! Port of `CodexUsageService.swift`. The main path reads the OAuth login
//! Codex saved in `%USERPROFILE%\.codex\auth.json` and asks
//! `chatgpt.com/backend-api/wham/usage` — not public API, what Codex's own
//! client calls. The saved token expires and nothing renews it for Pulse, so
//! when it is missing or refused this falls back to `codex app-server`, which
//! is signed in on its own terms and renews its credentials itself.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::app_server::{self, AppServerError};
use super::http::{self, Outcome};
use crate::model::{number, Provider, ProviderUsage, Reason, Route, UsageWindow, WindowKind};
use crate::paths;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

/// Which route to take. Pinning reports a failure instead of quietly
/// answering from elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    #[default]
    Automatic,
    Endpoint,
    /// `codex app-server` only.
    Tooling,
}

pub async fn fetch(source: Source) -> ProviderUsage {
    if source == Source::Tooling {
        return via_app_server().await.recording(Route::AppServer);
    }

    let outcome = match load_credentials() {
        Some((token, account)) => {
            let authorization = format!("Bearer {token}");
            let mut headers = vec![("Authorization", authorization.as_str())];
            // Sent only when there is one: an empty header names no account,
            // and the service is free to answer for whichever it likes.
            if !account.is_empty() {
                headers.push(("ChatGPT-Account-Id", account.as_str()));
            }
            http::get_json(USAGE_URL, &headers).await
        }
        None => Outcome::NeedsFreshCredentials,
    };

    match outcome {
        Outcome::Ok(root) => parse_usage_response(&root).recording(Route::Endpoint),
        Outcome::NeedsFreshCredentials if source == Source::Endpoint => {
            ProviderUsage::unavailable(Provider::Codex, Reason::CodexSignInRequired).recording(Route::Endpoint)
        }
        Outcome::NeedsFreshCredentials => via_app_server().await.recording(Route::AppServer),
        Outcome::Failed(reason) => ProviderUsage::unavailable(Provider::Codex, reason).recording(Route::Endpoint),
    }
}

/// Presence only — never the contents.
pub fn is_installed() -> bool {
    paths::codex_dir().exists()
}

/// Where Codex keeps its login.
pub fn credentials_file() -> PathBuf {
    paths::codex_dir().join("auth.json")
}

fn load_credentials() -> Option<(String, String)> {
    let text = std::fs::read_to_string(credentials_file()).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    let tokens = root.get("tokens")?;
    let token = tokens.get("access_token")?.as_str()?.to_string();
    let account = tokens.get("account_id").and_then(Value::as_str).unwrap_or_default().to_string();
    Some((token, account))
}

async fn via_app_server() -> ProviderUsage {
    match app_server::rate_limits().await {
        Ok(result) => parse_app_server_response(&result),
        // Neither route is open: no usable token, and no CLI to ask.
        Err(AppServerError::NotFound) => ProviderUsage::unavailable(Provider::Codex, Reason::CodexSignInRequired),
        Err(AppServerError::StartFailed) => ProviderUsage::unavailable(Provider::Codex, Reason::CodexServerFailed),
        Err(AppServerError::TimedOut) => ProviderUsage::unavailable(Provider::Codex, Reason::Unreachable),
        Err(AppServerError::Server(message)) => {
            let text = message.to_lowercase();
            let is_auth = ["auth", "login", "sign in", "unauthor", "credential"].iter().any(|w| text.contains(w));
            ProviderUsage::unavailable(
                Provider::Codex,
                if is_auth { Reason::CodexSignInRequired } else { Reason::ServerError },
            )
        }
    }
}

// MARK: - The endpoint's shape

pub fn parse_usage_response(root: &Value) -> ProviderUsage {
    let mut windows = Vec::new();

    let spend_reached = root
        .get("spend_control")
        .and_then(|s| s.get("reached"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Account-wide limits, which the server leaves unnamed. The flags
    // describe the *group*, so they are pinned to the window actually up
    // against its limit rather than smeared across every window in it.
    if let Some(limit) = root.get("rate_limit") {
        let reached_type = root.get("rate_limit_reached_type").is_some_and(|v| !v.is_null());
        windows.extend(marking_spent(
            http_windows(limit, "codex", None),
            is_group_spent(limit) || reached_type || spend_reached,
        ));
    }

    // Then per-model limits, which it does name.
    for extra in root.get("additional_rate_limits").and_then(Value::as_array).into_iter().flatten() {
        let Some(limit) = extra.get("rate_limit") else { continue };
        let label = extra.get("limit_name").and_then(Value::as_str);
        let key = extra
            .get("metered_feature")
            .and_then(Value::as_str)
            .or(label)
            .unwrap_or("extra");
        windows.extend(marking_spent(http_windows(limit, key, label), is_group_spent(limit)));
    }

    let credits = root.get("credits");
    let credit_balance = if credits.and_then(|c| c.get("unlimited")).and_then(Value::as_bool) == Some(true) {
        None
    } else {
        credits.and_then(|c| c.get("balance")).and_then(balance_text)
    };

    ProviderUsage::live(
        Provider::Codex,
        windows,
        root.get("plan_type").and_then(Value::as_str).map(plan_name),
        credit_balance,
    )
}

/// `primary_window` and `secondary_window` are not tied to particular
/// durations — ChatGPT Pro has no 5-hour limit at all — so a window's kind
/// comes from its length, never from the slot it arrived in.
fn http_windows(limit: &Value, id_prefix: &str, scope: Option<&str>) -> Vec<UsageWindow> {
    ["primary_window", "secondary_window"]
        .into_iter()
        .filter_map(|slot| {
            let node = limit.get(slot)?;
            let percent = number(node.get("used_percent"))?;
            let seconds = number(node.get("limit_window_seconds")).map(|s| s as i64);
            Some(UsageWindow {
                id: format!("{id_prefix}.{slot}"),
                kind: seconds.map_or(WindowKind::Other, kind),
                scope: scope.map(str::to_string),
                used_fraction: percent / 100.0,
                window_seconds: seconds.unwrap_or(0),
                resets_at: number(node.get("reset_at")).map(|s| (s * 1000.0) as i64),
                is_exhausted: false,
            })
        })
        .collect()
}

// MARK: - The app server's shape

/// `account/rateLimits/read`, which names its fields differently.
pub fn parse_app_server_response(result: &Value) -> ProviderUsage {
    let mut groups: Vec<(String, &Value)> = match result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        Some(by_id) => by_id.iter().map(|(k, v)| (k.clone(), v)).collect(),
        None => result
            .get("rateLimits")
            .map(|g| vec![("codex".to_string(), g)])
            .unwrap_or_default(),
    };

    // Unnamed account-wide group first, then named per-model ones in a
    // stable order, so rows don't jump around between refreshes.
    groups.sort_by(|(lk, lv), (rk, rv)| {
        let l_named = lv.get("limitName").and_then(Value::as_str).is_some();
        let r_named = rv.get("limitName").and_then(Value::as_str).is_some();
        l_named.cmp(&r_named).then_with(|| lk.cmp(rk))
    });

    // The server's own word on whether ordinary usage may still be spent.
    // `null` means unavailable, which is explicitly not "no".
    let ordinary_refused = result.get("ordinaryUsageAllowed").and_then(Value::as_bool) == Some(false);

    let mut windows = Vec::new();
    let mut plan: Option<String> = None;
    let mut credits: Option<Option<String>> = None;

    for (key, group) in groups {
        let scope = group.get("limitName").and_then(Value::as_str);
        let of_this_group: Vec<UsageWindow> = ["primary", "secondary"]
            .into_iter()
            .filter_map(|slot| {
                let node = group.get(slot)?;
                let percent = number(node.get("usedPercent"))?;
                let minutes = number(node.get("windowDurationMins")).map(|m| m as i64);
                Some(UsageWindow {
                    id: format!("{key}.{slot}"),
                    kind: minutes.map_or(WindowKind::Other, |m| kind(m * 60)),
                    scope: scope.map(str::to_string),
                    used_fraction: percent / 100.0,
                    window_seconds: minutes.unwrap_or(0) * 60,
                    resets_at: number(node.get("resetsAt")).map(|s| (s * 1000.0) as i64),
                    is_exhausted: false,
                })
            })
            .collect();

        // This group's windows, not every group's: the flag sits on the
        // snapshot that holds them.
        let spent = group.get("spendControlReached").and_then(Value::as_bool) == Some(true)
            || group.get("rateLimitReachedType").is_some_and(|v| !v.is_null())
            || ordinary_refused;
        windows.extend(marking_spent(of_this_group, spent));

        if plan.is_none() {
            plan = group.get("planType").and_then(Value::as_str).map(plan_name);
        }
        if credits.is_none() {
            if let Some(node) = group.get("credits") {
                credits = Some(if node.get("unlimited").and_then(Value::as_bool) == Some(true) {
                    None
                } else {
                    node.get("balance").and_then(balance_text)
                });
            }
        }
    }

    ProviderUsage::live(Provider::Codex, windows, plan, credits.flatten())
}

// MARK: - Shared

fn is_group_spent(limit: &Value) -> bool {
    limit.get("limit_reached").and_then(Value::as_bool) == Some(true)
        || limit.get("allowed").and_then(Value::as_bool) == Some(false)
}

/// Codex reports "limit reached" for a whole group, but only one of its
/// windows is the reason. Flagging the fullest keeps the claim as precise as
/// the data allows.
fn marking_spent(mut windows: Vec<UsageWindow>, spent: bool) -> Vec<UsageWindow> {
    if !spent {
        return windows;
    }
    if let Some(fullest) = windows
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.used_fraction.total_cmp(&b.1.used_fraction))
        .map(|(i, _)| i)
    {
        windows[fullest].is_exhausted = true;
    }
    windows
}

fn kind(seconds: i64) -> WindowKind {
    match seconds {
        18_000 => WindowKind::FiveHour,
        604_800 => WindowKind::Weekly,
        _ => WindowKind::Other,
    }
}

/// The balance arrives as a string; a number is taken too rather than lost.
fn balance_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// What the plan is actually called. `prolite` is the 5× Pro tier. Anything
/// unrecognised is passed through rather than blanked.
pub fn plan_name(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "free" => "Free",
        "go" => "Go",
        "plus" => "Plus",
        "pro" => "Pro",
        "prolite" => "Pro 5x",
        "team" => "Team",
        "business" => "Business",
        "enterprise" => "Enterprise",
        "edu" => "Edu",
        _ => return raw.to_string(),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_kinds_come_from_length_and_spent_sits_on_the_fullest() {
        let root = json!({
            "plan_type": "prolite",
            "rate_limit": {
                "allowed": true,
                "limit_reached": true,
                "primary_window": { "used_percent": 100, "limit_window_seconds": 18000, "reset_at": 1893456000 },
                "secondary_window": { "used_percent": 40, "limit_window_seconds": 604800, "reset_at": 1893456000 }
            },
            "additional_rate_limits": [{
                "limit_name": "GPT-5-Codex-Spark",
                "metered_feature": "codex_spark",
                "rate_limit": { "primary_window": { "used_percent": 3, "limit_window_seconds": 604800 } }
            }],
            "credits": { "unlimited": false, "balance": "12.5" }
        });
        let usage = parse_usage_response(&root);
        assert_eq!(usage.plan.as_deref(), Some("Pro 5x"));
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].kind, WindowKind::FiveHour);
        assert!(usage.windows[0].is_exhausted);
        assert!(!usage.windows[1].is_exhausted);
        assert_eq!(usage.windows[1].kind, WindowKind::Weekly);
        assert_eq!(usage.windows[0].resets_at, Some(1_893_456_000_000));
        assert_eq!(usage.windows[2].id, "codex_spark.primary_window");
        assert_eq!(usage.windows[2].scope.as_deref(), Some("GPT-5-Codex-Spark"));
        assert_eq!(usage.credit_balance.as_deref(), Some("12.5"));
    }

    #[test]
    fn pro_plan_with_only_a_weekly_window() {
        let root = json!({
            "plan_type": "pro",
            "rate_limit": { "primary_window": { "used_percent": 7, "limit_window_seconds": 604800 } },
            "credits": { "unlimited": true, "balance": "0" }
        });
        let usage = parse_usage_response(&root);
        assert_eq!(usage.windows.len(), 1);
        assert_eq!(usage.windows[0].kind, WindowKind::Weekly);
        assert_eq!(usage.credit_balance, None);
    }

    #[test]
    fn app_server_groups_are_ordered_and_marked_separately() {
        let result = json!({
            "rateLimitsByLimitId": {
                "zmodel": { "limitName": "Spark", "rateLimitReachedType": "primary",
                            "primary": { "usedPercent": 90, "windowDurationMins": 300, "resetsAt": 1893456000 } },
                "codex": { "planType": "plus",
                           "primary": { "usedPercent": 95, "windowDurationMins": 300 },
                           "secondary": { "usedPercent": 20, "windowDurationMins": 10080 } }
            }
        });
        let usage = parse_app_server_response(&result);
        assert_eq!(usage.plan.as_deref(), Some("Plus"));
        let ids: Vec<&str> = usage.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["codex.primary", "codex.secondary", "zmodel.primary"]);
        // Only the named group reported its limit reached.
        assert!(!usage.windows[0].is_exhausted);
        assert!(usage.windows[2].is_exhausted);
        assert_eq!(usage.windows[1].kind, WindowKind::Weekly);
    }

    #[test]
    fn ordinary_usage_refused_marks_every_group() {
        let result = json!({
            "ordinaryUsageAllowed": false,
            "rateLimits": { "primary": { "usedPercent": 10, "windowDurationMins": 300 },
                            "secondary": { "usedPercent": 60, "windowDurationMins": 10080 } }
        });
        let usage = parse_app_server_response(&result);
        assert!(!usage.windows[0].is_exhausted);
        assert!(usage.windows[1].is_exhausted);
    }

    #[test]
    fn unknown_plans_pass_through() {
        assert_eq!(plan_name("business"), "Business");
        assert_eq!(plan_name("ultra_2027"), "ultra_2027");
    }
}
