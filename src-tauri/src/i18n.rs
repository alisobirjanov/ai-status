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
        "quit" => if russian { "Выйти из Pulse" } else { "Quit Pulse" },
        "settingsTitle" => if russian { "Настройки Pulse" } else { "Pulse Settings" },
        // Beside a ring's two figures: the 5-hour limit and the weekly one.
        "fiveHourLetter" => if russian { "ч" } else { "h" },
        "weeklyLetter" => if russian { "н" } else { "w" },
        // A new version of Pulse. `{0}` is the version.
        "installUpdate" => if russian { "Установить обновление {0}…" } else { "Install Update {0}…" },
        "updateTitle" => if russian { "Вышла версия Pulse {0}" } else { "Pulse {0} is available" },
        "updateBody" => if russian {
            "Установить её можно из меню в трее или в настройках Pulse."
        } else {
            "Install it from the tray menu or from Pulse Settings."
        },
        _ => "",
    }
}
