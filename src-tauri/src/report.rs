//! `Pulse.exe --json`: what the running app last banked, for status lines
//! and scripts. Same contract as the macOS app (`Docs/json-output.md`):
//!
//! - It prints the cache and **never fetches**. Every account carries
//!   `observedAt` and `ageSeconds`; deciding what is too old is the reader's.
//! - It reads and never writes.
//! - Nothing in it is translated: `kind` is a flat token, not a window name.

use serde_json::{json, Value};

use crate::cache::Cache;
use crate::model::{now_ms, percent_value, provider_of, ProviderUsage, Route, UsageWindow, WindowKind};
use crate::settings::Settings;

pub fn render() -> String {
    let settings = Settings::load();
    let mut cache = Cache::new();
    let now = now_ms();

    // An installation that has never chosen prints an empty rail, not a
    // guess at what would be switched on.
    let enabled = if settings.has_chosen { settings.monitored() } else { Vec::new() };
    let accounts: Vec<Value> = enabled
        .into_iter()
        .filter_map(|id| {
            let provider = provider_of(&id)?;
            let label = settings.account_labels.get(&id).map_or(provider.display_name(), String::as_str);
            let mut account = json!({
                "id": id,
                "provider": provider.id(),
                "name": provider.display_name(),
                "label": label,
                "windows": [],
            });
            if let Some(usage) = cache.reading(&id) {
                fill(&mut account, &usage, now);
            }
            Some(account)
        })
        .collect();

    let report = json!({ "generatedAt": iso(now), "accounts": accounts });
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
}

fn fill(account: &mut Value, usage: &ProviderUsage, now: i64) {
    let object = account.as_object_mut().expect("an object");
    if let Some(plan) = &usage.plan {
        object.insert("plan".into(), json!(plan));
    }
    if let Some(balance) = &usage.credit_balance {
        object.insert("creditBalance".into(), json!(balance));
    }
    if let Some(observed) = usage.observed_at {
        object.insert("observedAt".into(), json!(iso(observed)));
        object.insert("ageSeconds".into(), json!((now - observed) / 1000));
    }
    if let Some(origin) = usage.origin {
        let token = match origin {
            Route::Endpoint => "endpoint",
            Route::AppServer => "appServer",
            Route::ClaudeCode => "claudeCode",
        };
        object.insert("source".into(), json!(token));
    }
    if let Some(headline) = usage.headline() {
        object.insert(
            "headline".into(),
            json!({
                "windowId": headline.id,
                "usedPercent": percent_value(headline.used_fraction),
                "exhausted": headline.is_exhausted,
                "resetsAt": headline.resets_at.map(iso),
            }),
        );
    }
    object.insert("windows".into(), Value::Array(usage.windows.iter().map(window).collect()));
}

fn window(window: &UsageWindow) -> Value {
    let kind = match window.kind {
        WindowKind::FiveHour => "fiveHour".to_string(),
        WindowKind::Weekly => "weekly".to_string(),
        WindowKind::Spend => "spend".to_string(),
        WindowKind::Other => format!("other:{}", window.window_seconds),
    };
    json!({
        "id": window.id,
        "kind": kind,
        "scope": window.scope,
        "usedPercent": percent_value(window.used_fraction),
        "usedFraction": window.used_fraction,
        "exhausted": window.is_exhausted,
        "windowSeconds": window.window_seconds,
        "reportsLength": window.window_seconds > 0,
        "resetsAt": window.resets_at.map(iso),
    })
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}
