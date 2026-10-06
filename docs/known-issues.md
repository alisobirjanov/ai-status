# Известные проблемы

Здесь собраны проблемы, которые уже найдены и разобраны, но пока сознательно
не исправлены. Для каждой описано, как её увидеть, почему она возникает, и
приведено готовое решение, чтобы вернуться к нему, если проблема начнёт мешать.

## Панель уходит под окна «поверх всех»

**Статус:** не исправлено. Решение написано и проверено, но отложено: пока
проблема не мешает, поведение панели решили не менять.

### Как это выглядит

Иногда панель оказывается под другим окном и сама наверх не возвращается. Так
бывает не с любым окном, а только с теми, которые тоже закреплены «поверх
всех»: видео в режиме «картинка в картинке», окно звонка, окно, закреплённое
через PowerToys (Win+Ctrl+T), диспетчер задач с включённым «Поверх остальных
окон».

### Почему

Панель создаётся с `always_on_top(true)` (`WS_EX_TOPMOST`), и этот флаг у неё
есть. Но окон «поверх всех» может быть несколько, и между собой они
упорядочены по активации: выше то, которое активировали последним.

Сама панель активной не становится никогда. Она создана с `focusable(false)`
(`WS_EX_NOACTIVATE`), чтобы щелчок по кольцу не отбирал фокус у окна, где
человек печатает. Поэтому любое другое окно «поверх всех», по которому
щёлкнули, ложится выше панели и там остаётся: у панели не бывает момента, когда
Windows подняла бы её обратно.

Прозрачность для кликов здесь ни при чём. Её переключает поток, который следит
за курсором (`set_ignore_cursor_events`), и при этом `tao` заново записывает
стили окна. Но флаг `WS_EX_TOPMOST` так не снимается, и порядок окон не
меняется.

### Как воспроизвести

При запущенном Dipstick выполните в Windows PowerShell 5.1:

```powershell
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Z {
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  // Место окна среди видимых, 0 — самое верхнее.
  public static int Rank(IntPtr w) {
    IntPtr h = GetWindow(w, 0); int i = 0;
    while (h != IntPtr.Zero) { if (h == w) return i; if (IsWindowVisible(h)) i++; h = GetWindow(h, 2); }
    return -1; }
}
'@
$panel = [Z]::FindWindow("Tauri Window", "Dipstick")
$form = New-Object System.Windows.Forms.Form
$form.TopMost = $true; $form.Text = "test"
$form.Show(); $form.Activate(); Start-Sleep -Milliseconds 800
"тестовое окно: $([Z]::Rank($form.Handle)), панель: $([Z]::Rank($panel))"
$form.Close()
```

Тестовое окно окажется выше панели (его номер меньше) и останется выше.

### Решение

Поднимать панель обратно наверх, не активируя её, каждый раз, когда в Windows
меняется активное окно:

1. На главном потоке ставится хук `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`.
   При смене активного окна он вызывает
   `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE | …)`.
2. **Одного подъёма мало.** Только что открывшееся окно может выставить себе
   «поверх всех» уже после того, как стало активным (так делает WinForms и
   многие программы), и панель снова оказывалась под ним. Поэтому ещё полторы
   секунды после смены окна панель поднимается каждые 200 мс. Это делает поток,
   который и так 30 раз в секунду следит за курсором, отдельного таймера нет.
3. **Пока открыто меню, панель не поднимается.** Это проверяется через
   `GetGUIThreadInfo` (флаг `GUI_INMENUMODE` и соседние) и у активной
   программы, и у самой панели. Меню — тоже окно «поверх всех», и поднятая
   панель закрыла бы часть меню. Её собственное меню по правому щелчку
   открывается прямо над ней.
4. Раз в секунду проверяется, не пропал ли сам флаг `WS_EX_TOPMOST`; если
   пропал, он возвращается.

#### Новый файл `src-tauri/src/topmost.rs`

```rust
//! Keeping the panel above every other window.
//!
//! `always_on_top` makes the panel a topmost window, but topmost windows are
//! ordered among themselves by activation: whichever was activated last is on
//! top. The panel never activates — it is WS_EX_NOACTIVATE so that clicking a
//! ring does not take focus — so any other topmost window the reader clicks
//! (a video's picture-in-picture, a call window, a window pinned with
//! PowerToys) lands above it and stays there. Reproduced: a topmost window
//! activated over the rail sat on top of it, and the rail never came back.
//!
//! So whenever the foreground window changes, the panel is lifted back to the
//! top — without activating it — and a watchdog restores the topmost flag if
//! anything ever clears it.
//!
//! **Once is not enough.** A window that has just opened can make itself
//! topmost *after* it became the foreground window (WinForms does, and so do
//! many real apps): lifted at the moment of the change, the panel went back
//! under it a moment later. So it is lifted again every 200ms for a second and
//! a half after each change, by the pointer thread that is running anyway.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::WebviewWindow;

/// The panel's window handle, for the hook, which cannot be handed state.
static PANEL: AtomicIsize = AtomicIsize::new(0);
/// A hidden panel is left where it is.
static SHOWN: AtomicBool = AtomicBool::new(false);
/// Until when (Unix ms) to keep lifting it after the foreground changed.
static LIFT_UNTIL: AtomicU64 = AtomicU64::new(0);
const LIFT_FOR_MS: u64 = 1500;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub fn track(window: &WebviewWindow) {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        PANEL.store(hwnd.0 as isize, Ordering::SeqCst);
    }
    #[cfg(not(windows))]
    let _ = window;
}

pub fn set_shown(shown: bool) {
    SHOWN.store(shown, Ordering::SeqCst);
    if shown {
        raise();
    }
}

#[cfg(windows)]
mod win {
    use std::sync::atomic::Ordering;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetWindowLongPtrW, GetWindowThreadProcessId, SetWindowPos, EVENT_SYSTEM_FOREGROUND,
        GUITHREADINFO, GUI_INMENUMODE, GUI_POPUPMENUMODE, GUI_SYSTEMMENUMODE, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, WINEVENT_OUTOFCONTEXT,
        WINEVENT_SKIPOWNPROCESS, WS_EX_TOPMOST,
    };

    use super::{now_ms, LIFT_FOR_MS, LIFT_UNTIL, PANEL, SHOWN};

    fn panel() -> Option<HWND> {
        let handle = PANEL.load(Ordering::SeqCst);
        (handle != 0 && SHOWN.load(Ordering::SeqCst)).then_some(handle as HWND)
    }

    /// Whether a menu is open in the thread: a menu is a topmost window too,
    /// and lifting the panel over one that reaches across the rail — its own
    /// right-click menu, always — would hide part of it.
    fn in_menu(thread: u32) -> bool {
        let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        // SAFETY: fills a struct we own, sized as it asks.
        let known = unsafe { GetGUIThreadInfo(thread, &mut info) } != 0;
        known && info.flags & (GUI_INMENUMODE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE) != 0
    }

    /// To the top of the topmost band. Never activates, never moves or
    /// resizes, and does not wait for the window's thread when called from
    /// another one. Not while a menu is open, here or in the app in front.
    pub fn raise() {
        let Some(hwnd) = panel() else { return };
        // SAFETY: a thread id query on our own window.
        let own_thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
        // Thread 0 is whichever thread owns the foreground window.
        if in_menu(0) || in_menu(own_thread) {
            return;
        }
        // SAFETY: a handle to our own window; SetWindowPos tolerates one that
        // has since been destroyed by failing.
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS,
            );
        }
    }

    pub fn restore_if_lost() {
        let Some(hwnd) = panel() else { return };
        // SAFETY: reads a style word of our own window.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        if style & WS_EX_TOPMOST == 0 {
            raise();
        }
    }

    unsafe extern "system" fn foreground_changed(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        raise();
        LIFT_UNTIL.store(now_ms() + LIFT_FOR_MS, Ordering::SeqCst);
    }

    /// Must run on a thread that pumps messages — the main thread: an
    /// out-of-context hook is delivered through its queue.
    pub fn install() {
        // SAFETY: the callback is a plain function that lives as long as the
        // process; the hook is never removed.
        unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(foreground_changed),
                0,
                0,
                // Our own Settings window becoming active changes nothing:
                // it is not topmost, so the panel is still above it.
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
        }
    }
}

/// Lift the panel back above whatever took the top when the foreground
/// window changes. Call once, from the main thread.
pub fn install() {
    #[cfg(windows)]
    win::install();
}

pub fn raise() {
    #[cfg(windows)]
    win::raise();
}

/// Called on every step of the pointer thread (about 30 a second): lifts the
/// panel every 200ms while a foreground change is recent, and once a second
/// puts the topmost flag back if it has gone.
pub fn tick(step: u32) {
    #[cfg(windows)]
    {
        if step % 6 == 0 && now_ms() < LIFT_UNTIL.load(Ordering::SeqCst) {
            win::raise();
        }
        if step % 30 == 0 {
            win::restore_if_lost();
        }
    }
    #[cfg(not(windows))]
    let _ = step;
}
```

#### Что поменять в остальных файлах

`src-tauri/Cargo.toml` — добавить возможность `Win32_UI_Accessibility`, в ней
находится `SetWinEventHook`:

```toml
windows-sys = { version = "0.61.2", features = ["Win32_Foundation", "Win32_System_Console", "Win32_UI_WindowsAndMessaging", "Win32_UI_Input_KeyboardAndMouse", "Win32_UI_Accessibility"] }
```

`src-tauri/src/lib.rs` — подключить модуль и поставить хук в `setup`. Это
главный поток, а хуку нужен поток с очередью сообщений:

```rust
mod topmost;
// …
        .setup(|app| {
            // …
            topmost::install();
            panel::sync(&handle);
```

`src-tauri/src/panel.rs`:

```rust
// sync(): панель спрятана
        panel.shown.store(false, Ordering::SeqCst);
        crate::topmost::set_shown(false);

// sync(): панель показана
    let _ = window.show();
    panel.shown.store(true, Ordering::SeqCst);
    crate::topmost::set_shown(true);

// create(): запомнить окно
    let _ = window.set_ignore_cursor_events(true);
    crate::topmost::track(&window);

// start_pointer_watch(): счётчик шагов и вызов на каждом шаге
    std::thread::spawn(move || {
        let mut inside = false;
        let mut ticks: u32 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            ticks = ticks.wrapping_add(1);
            crate::topmost::tick(ticks);
            // …
```

На компьютере, где решение писалось, оно лежит в локальной ветке
`keep-panel-on-top` и возвращается командой `git cherry-pick keep-panel-on-top`.
На GitHub этой ветки нет.

### Что проверено

На запущенной копии (Pulse Dev) панель оставалась наверху во всех случаях:

- открылось новое окно «поверх всех»;
- по такому окну щёлкнули ещё раз;
- активировали другое окно;
- окно заново выставило себе «поверх всех», не меняя фокуса;
- через секунду после переключения окон открыли меню поверх панели — меню
  осталось выше панели.

**Нагрузка.** Замерялся основной процесс, по 30 секунд, рядом с версией без
решения:

| Случай                              | С решением                           | Без решения     |
| ----------------------------------- | ------------------------------------ | --------------- |
| Покой                               | 16 мс процессора (0,05% одного ядра) | 94 мс (0,31%)   |
| Окно переключается каждую секунду   | 47 мс (0,16%)                        | 47 мс (0,16%)   |

Разница в покое — это разброс: у версии без решения в это время шло
обновление данных. Проверка раз в секунду — одно чтение флага окна, а подъём
запускает событие Windows, а не постоянный опрос.

### Что учесть, если включать

- Панель станет видна и **поверх полноэкранных программ** — видео, игр. Если это
  мешает, стоит сразу добавить настройку «Прятать поверх полноэкранных
  программ».
- Если другая программа удерживает себя наверху так же, две программы будут
  поднимать себя по очереди. Повторные подъёмы ограничены полутора секундами
  после смены окна, поэтому бесконечного перетягивания не будет.
