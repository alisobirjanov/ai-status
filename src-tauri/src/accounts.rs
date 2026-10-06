//! Claude accounts beyond the one Claude Code itself is signed in to. One is
//! added by signing in in the browser — Claude Code does the signing in,
//! into a folder of Dipstick's own — and can then be named, signed in to again,
//! handed to Claude Code, and removed. Nobody types a command.

use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::model::{provider_of, slot_of, AccountId, Provider};
use crate::paths;
use crate::providers::claude_code::{self, SignInError};
use crate::providers::{self, claude};
use crate::settings::tidy_label;
use crate::store::{self, AppState, SignIn, SignInProblem, Switch, SwitchProblem};
use crate::switch::{self, Place};

/// Beyond the one Claude Code has: as many as a rail still fits a screen with.
pub const MAX_ADDED: usize = 4;

/// Sign in to a Claude account in the browser: a new one with `None`, or
/// `account` again once its login has gone.
pub fn sign_in(app: &AppHandle, account: Option<AccountId>) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    if let Some(id) = &account {
        if provider_of(id) != Some(Provider::ClaudeCode) || !settings.all_accounts().contains(id) {
            return;
        }
    }
    let cancelled = Arc::new(tokio::sync::Notify::new());
    let read;
    {
        let mut store = state.store.lock().unwrap();
        // One at a time: there is one browser, and one person in front of it.
        if store.sign_in.as_ref().is_some_and(|s| s.waiting) || store.switch.as_ref().is_some_and(|s| s.waiting) {
            return;
        }
        // The account in Claude Code is signed in to again there: that is
        // the login in use, not the one left in its folder.
        read = account.as_deref().map(|id| store.read_as(id).to_string());
        let problem = if account.is_none() && settings.claude_accounts.len() >= MAX_ADDED {
            Some(SignInProblem::TooMany)
        } else if !claude_code::is_available() {
            Some(SignInProblem::NoClaudeCode)
        } else {
            None
        };
        store.sign_in = Some(SignIn { account: account.clone(), waiting: problem.is_none(), problem, detail: None });
        store.sign_in_cancel = problem.is_none().then(|| cancelled.clone());
        if problem.is_some() {
            drop(store);
            store::emit_snapshot(app);
            return;
        }
    }
    store::emit_snapshot(app);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (id, folder) = match &read {
            Some(id) => (id.clone(), providers::folder_of(id)),
            None => {
                let slot = new_slot(&settings.claude_accounts);
                (format!("claudeCode#{slot}"), Some(paths::claude_account_dir(&slot)))
            }
        };
        // A new account has a folder of its own before Claude Code signs in to it.
        if let Some(folder) = &folder {
            let _ = std::fs::create_dir_all(folder);
        }
        let result = claude_code::sign_in(folder.as_deref(), &cancelled).await;
        finish(&app, account, id, folder, result);
    });
}

/// `asked` is the account the sign-in was for, `None` a new one; `id` the
/// login signed in to.
fn finish(app: &AppHandle, asked: Option<AccountId>, id: AccountId, folder: Option<PathBuf>, result: Result<(), SignInError>) {
    let is_new = asked.is_none();
    let state = app.state::<AppState>();
    let identity = claude::identity(folder.as_deref());
    let signed_in = claude::credentials_file(folder.as_deref()).is_file();
    let settings = state.settings.lock().unwrap().clone();

    let problem = match result {
        Ok(()) if !signed_in => Some((SignInProblem::Failed, None)),
        Ok(()) => {
            // The same account added twice is one account. The one Claude
            // Code is signed in to can be added too: Claude Code may change
            // accounts, and the one it leaves is then still read.
            let twice = identity.uuid.is_some()
                && slot_of(&id).is_some()
                && settings
                    .all_accounts()
                    .iter()
                    .filter(|other| **other != id && slot_of(other).is_some())
                    .any(|other| claude::identity(providers::folder_of(other).as_deref()).uuid == identity.uuid);
            twice.then_some((SignInProblem::AlreadyAdded, None))
        }
        Err(SignInError::Cancelled) => None,
        Err(SignInError::NotFound) => Some((SignInProblem::NoClaudeCode, None)),
        Err(SignInError::TimedOut) => Some((SignInProblem::TimedOut, None)),
        Err(SignInError::StartFailed) => Some((SignInProblem::Failed, None)),
        Err(SignInError::Failed(detail)) => Some((SignInProblem::Failed, detail)),
    };
    let added = is_new && signed_in && problem.is_none();
    let account = asked;

    {
        let mut store = state.store.lock().unwrap();
        store.sign_in_cancel = None;
        store.sign_in = problem.map(|(problem, detail)| SignIn { account, waiting: false, problem: Some(problem), detail });
        if added || (!is_new && signed_in) {
            store.note_identity(&id, identity);
        }
    }

    if is_new && !added {
        // Nothing of an account that wasn't added is left behind. Signed in to
        // one already there, its login is only let go of: signing it out
        // would be signing out the account that is.
        if let Some(folder) = &folder {
            discard(folder);
        }
        store::emit_snapshot(app);
        return;
    }
    if added {
        let slot = slot_of(&id).map(str::to_string).unwrap_or_default();
        // Read at once: that is what it was added for.
        crate::change_settings(app, |settings| {
            settings.claude_accounts.push(slot);
            let claude = Provider::ClaudeCode.id().to_string();
            if !settings.enabled.contains(&claude) {
                settings.enabled.insert(0, claude);
            }
            settings.has_chosen = true;
        });
        return;
    }
    store::emit_snapshot(app);
    store::refresh(app, &[id]);
}

/// Stop waiting for a sign-in, or put away why the last one didn't work.
pub fn cancel_sign_in(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut store = state.store.lock().unwrap();
    match store.sign_in_cancel.take() {
        // What is shown changes once Claude Code has gone.
        Some(cancelled) => cancelled.notify_one(),
        None => {
            store.sign_in = None;
            drop(store);
            store::emit_snapshot(app);
        }
    }
}

/// Remove an account Dipstick added: off the rail, its readings forgotten,
/// signed out and its folder gone. Claude Code's own login is not Dipstick's to
/// remove.
pub fn remove(app: &AppHandle, account: AccountId) {
    let Some(slot) = slot_of(&account).map(str::to_string) else { return };
    let Some(folder) = providers::folder_of(&account) else { return };
    let state = app.state::<AppState>();
    let in_claude_code;
    {
        let mut store = state.store.lock().unwrap();
        if store.switch.as_ref().is_some_and(|s| s.waiting) {
            return;
        }
        in_claude_code = store.mirrors(&account);
        if store.sign_in.as_ref().is_some_and(|s| s.account.as_deref() == Some(account.as_str())) {
            if let Some(cancelled) = store.sign_in_cancel.take() {
                cancelled.notify_one();
            }
            store.sign_in = None;
        }
        store.forget(&account);
    }
    crate::change_settings(app, |settings| settings.claude_accounts.retain(|s| *s != slot));
    tauri::async_runtime::spawn(async move {
        // Signing out ends a login wherever it is in use, and the one in
        // Claude Code is in use there: it is only let go of here.
        if !in_claude_code {
            claude_code::sign_out(&folder).await;
        }
        discard(&folder);
    });
}

/// Hand Claude Code an account's login, so Claude Code — in a terminal, in
/// VS Code, wherever it runs — is that account, without signing in again.
pub fn use_in_claude_code(app: &AppHandle, account: AccountId) {
    let Some(folder) = providers::folder_of(&account) else { return };
    let state = app.state::<AppState>();
    if !state.settings.lock().unwrap().all_accounts().contains(&account) {
        return;
    }
    {
        let mut store = state.store.lock().unwrap();
        if store.sign_in.as_ref().is_some_and(|s| s.waiting) || store.switch.as_ref().is_some_and(|s| s.waiting) || store.mirrors(&account) {
            return;
        }
        store.switch = Some(Switch { account: account.clone(), waiting: true, problem: None });
    }
    store::emit_snapshot(app);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // A login being read may be being renewed: what is copied now could
        // be gone in a moment. Nothing new is read until this is done.
        store::settle(&app).await;
        let outcome = hand_over(&app, &folder).await;
        finished_switch(&app, account, outcome);
    });
}

/// Where Claude Code's own login went, if it had one: back to the account
/// it is, or to a new one.
struct Back {
    account: AccountId,
    new_slot: Option<String>,
}

async fn hand_over(app: &AppHandle, folder: &Path) -> Result<Option<Back>, SwitchProblem> {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    // Whose Claude Code's login is: as noted beside it, and where it can be
    // asked, as the login itself says. Were they to differ, the tokens would
    // go back to the wrong account.
    let noted = claude::identity(None).uuid;
    if let (Some(noted), Some(owner)) = (&noted, claude::owner(None).await) {
        if *noted != owner {
            return Err(SwitchProblem::Unknown);
        }
    }
    let back = noted.map(|uuid| {
        let home = settings
            .claude_accounts
            .iter()
            .find(|slot| claude::identity(Some(&paths::claude_account_dir(slot))).uuid.as_deref() == Some(uuid.as_str()));
        match home {
            Some(slot) => Back { account: format!("claudeCode#{slot}"), new_slot: None },
            None => {
                let slot = new_slot(&settings.claude_accounts);
                Back { account: format!("claudeCode#{slot}"), new_slot: Some(slot) }
            }
        }
    });
    let place = back.as_ref().and_then(|back| providers::folder_of(&back.account)).map(|folder| Place::folder(&folder));
    let account = Place::folder(folder);
    let result = tauri::async_runtime::spawn_blocking(move || switch::hand_over(&Place::claude_code(), &account, place.as_ref())).await;
    match result {
        Ok(Ok(())) => Ok(back),
        outcome => {
            // A new folder that didn't take a login is not kept.
            if let Some(slot) = back.and_then(|back| back.new_slot) {
                discard(&paths::claude_account_dir(&slot));
            }
            Err(match outcome {
                Ok(Err(switch::Problem::NoLogin)) => SwitchProblem::NoLogin,
                Ok(Err(switch::Problem::NowhereBack)) => SwitchProblem::Unknown,
                _ => SwitchProblem::Failed,
            })
        }
    }
}

fn finished_switch(app: &AppHandle, account: AccountId, outcome: Result<Option<Back>, SwitchProblem>) {
    let state = app.state::<AppState>();
    let own = Provider::ClaudeCode.id().to_string();
    let back = match outcome {
        Ok(back) => back,
        Err(problem) => {
            state.store.lock().unwrap().switch = Some(Switch { account, waiting: false, problem: Some(problem) });
            store::emit_snapshot(app);
            state.wake.notify_one();
            return;
        }
    };
    {
        let mut store = state.store.lock().unwrap();
        // Each login takes its figures where it went.
        if let Some(back) = &back {
            store.copy_readings(&own, &back.account);
        }
        store.copy_readings(&account, &own);
        let mut moved = vec![own.clone(), account.clone()];
        moved.extend(back.as_ref().map(|back| back.account.clone()));
        store.reread_identities(&moved);
        store.switch = Some(Switch { account, waiting: false, problem: None });
    }
    if let Some(Back { account: kept, new_slot: Some(slot) }) = &back {
        // Claude Code's own account, kept in a folder now, keeps its place
        // at the top and its name.
        crate::change_settings(app, |settings| {
            settings.claude_accounts.insert(0, slot.clone());
            if let Some(label) = settings.account_labels.remove(&own) {
                settings.account_labels.insert(kept.clone(), label);
            }
        });
    }
    store::emit_snapshot(app);
    let mut ask = vec![own];
    ask.extend(back.map(|back| back.account));
    store::refresh(app, &ask);
    state.wake.notify_one();
}

/// Put away how the last switch went.
pub fn dismiss_switch(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut store = state.store.lock().unwrap();
        if store.switch.as_ref().is_some_and(|s| s.waiting) {
            return;
        }
        store.switch = None;
    }
    store::emit_snapshot(app);
}

/// Name an account, or with an empty name go back to the one it had.
pub fn rename(app: &AppHandle, account: AccountId, label: String) {
    let label = tidy_label(&label);
    crate::change_settings(app, |settings| {
        if label.is_empty() {
            settings.account_labels.remove(&account);
        } else {
            settings.account_labels.insert(account.clone(), label.clone());
        }
    });
}

/// Only ever a folder of Dipstick's own accounts.
fn discard(folder: &Path) {
    if folder.parent() == Some(paths::data_dir().join("accounts").as_path()) {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// A slot no account has yet, and no folder either.
fn new_slot(taken: &[String]) -> String {
    loop {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_i64(crate::model::now_ms());
        let slot = format!("{:08x}", hasher.finish() as u32);
        if !taken.contains(&slot) && !paths::claude_account_dir(&slot).exists() {
            return slot;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_slot_names_a_folder() {
        let slot = new_slot(&[]);
        assert!(crate::model::is_slot(&slot));
        assert_ne!(new_slot(std::slice::from_ref(&slot)), slot);
    }
}
