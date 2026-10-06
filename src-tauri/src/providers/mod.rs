//! One module per product, plus what they share.

pub mod app_server;
pub mod claude;
pub mod claude_code;
pub mod codex;
pub mod http;

use std::path::PathBuf;

use crate::model::{provider_of, slot_of, Provider, ProviderUsage, Reason};
use crate::paths;
use crate::settings::Settings;

/// The folder an added Claude account's login is kept in; `None` for a
/// provider's own login, wherever the provider keeps it.
pub fn folder_of(account: &str) -> Option<PathBuf> {
    slot_of(account).map(paths::claude_account_dir)
}

pub async fn fetch(account: &str, settings: &Settings) -> ProviderUsage {
    match provider_of(account) {
        Some(Provider::ClaudeCode) => claude::fetch(folder_of(account).as_deref()).await,
        Some(Provider::Codex) => codex::fetch(settings.codex_source).await,
        None => ProviderUsage::unavailable(Provider::ClaudeCode, Reason::NotChecked),
    }
}

/// Whether the product's own folder exists — or an added account's. Presence
/// only: nothing is opened or parsed, and detection never enables anything.
pub fn is_installed(account: &str) -> bool {
    match (provider_of(account), folder_of(account)) {
        (_, Some(folder)) => folder.is_dir(),
        (Some(Provider::ClaudeCode), None) => claude::is_installed(),
        (Some(Provider::Codex), None) => codex::is_installed(),
        (None, None) => false,
    }
}

/// The login an account is read with. Read, never written.
pub fn credentials_file(account: &str) -> PathBuf {
    match provider_of(account) {
        Some(Provider::Codex) => codex::credentials_file(),
        _ => claude::credentials_file(folder_of(account).as_deref()),
    }
}
