//! One module per product, plus what they share.

pub mod app_server;
pub mod claude;
pub mod codex;
pub mod http;

use std::path::PathBuf;

use crate::model::{Provider, ProviderUsage};
use crate::settings::Settings;

pub async fn fetch(provider: Provider, settings: &Settings) -> ProviderUsage {
    match provider {
        Provider::ClaudeCode => claude::fetch().await,
        Provider::Codex => codex::fetch(settings.codex_source).await,
    }
}

/// Whether the product's own folder exists. Presence only: nothing is opened
/// or parsed, and detection never enables anything.
pub fn is_installed(provider: Provider) -> bool {
    match provider {
        Provider::ClaudeCode => claude::is_installed(),
        Provider::Codex => codex::is_installed(),
    }
}

/// The login a provider is read with. Read, never written.
pub fn credentials_file(provider: Provider) -> PathBuf {
    match provider {
        Provider::ClaudeCode => claude::credentials_file(),
        Provider::Codex => codex::credentials_file(),
    }
}
