//! The few strings drawn by Rust rather than the page: tray and menus.
//! English and Russian, chosen by the system language.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    English,
    Russian,
}

fn language() -> Language {
    static LANGUAGE: OnceLock<Language> = OnceLock::new();
    *LANGUAGE.get_or_init(|| match sys_locale::get_locale() {
        Some(locale) if locale.to_lowercase().starts_with("ru") => Language::Russian,
        _ => Language::English,
    })
}

pub fn text(key: &str) -> &'static str {
    let russian = language() == Language::Russian;
    match key {
        "showPanel" => if russian { "Показывать панель" } else { "Show Panel" },
        "hidePanel" => if russian { "Скрыть панель" } else { "Hide Panel" },
        "refresh" => if russian { "Обновить сейчас" } else { "Refresh Now" },
        "settings" => if russian { "Настройки…" } else { "Settings…" },
        "quit" => if russian { "Выйти из Dipstick" } else { "Quit Dipstick" },
        "settingsTitle" => if russian { "Настройки Dipstick" } else { "Dipstick Settings" },
        // Beside a ring's two figures: the 5-hour limit and the weekly one.
        "fiveHourLetter" => if russian { "ч" } else { "h" },
        "weeklyLetter" => if russian { "н" } else { "w" },
        // A new version of Dipstick. `{0}` is the version.
        "installUpdate" => if russian { "Установить обновление {0}…" } else { "Install Update {0}…" },
        "updateTitle" => if russian { "Вышла версия Dipstick {0}" } else { "Dipstick {0} is available" },
        "updateBody" => if russian {
            "Установить её можно из меню в трее или в настройках Dipstick."
        } else {
            "Install it from the tray menu or from Dipstick Settings."
        },
        // The Claude account in use has run out of a limit. In the title
        // `{0}` is the account with room; in the body `{0}` is the one in
        // use, `{1}` the limit it ran out of, `{2}` the free one's figures.
        "freeTitle" => if russian { "{0} свободен" } else { "{0} has room" },
        "freeBody" => if russian { "{0}: {1} исчерпан. Свободный: {2}." } else { "{0} hit its {1}. This one: {2}." },
        "fiveHourLimit" => if russian { "5-часовой лимит" } else { "5-hour limit" },
        "weeklyLimit" => if russian { "недельный лимит" } else { "weekly limit" },
        "fiveHour" => if russian { "5 ч" } else { "5-hour" },
        "weekly" => if russian { "неделя" } else { "weekly" },
        _ => "",
    }
}
