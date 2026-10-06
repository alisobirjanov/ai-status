# Development

Dipstick for Windows is a [Tauri 2](https://tauri.app) app. Rust (`src-tauri/`)
reads credentials, asks the endpoints, keeps the cache and owns the windows;
the TypeScript pages (`src/`) only draw what Rust sends them.

## Prerequisites

- **Node.js** 20.19 or newer
- **Rust** stable, through [rustup](https://rustup.rs)
- **Visual Studio Build Tools** with the *Desktop development with C++*
  workload (Rust's MSVC linker and the Windows SDK)
- **WebView2** — already part of Windows 11

With winget:

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Then, in the checkout:

```powershell
npm ci
npm test        # Rust unit tests, then the TypeScript type check
```

## Trying a change: Dipstick Dev

Most people working on Dipstick also use it. A build of the checkout must not
move their rail, rewrite their settings or replace their install, so changes
are tried in **Dipstick Dev**, a copy that runs beside the installed Dipstick:

```powershell
npm run app        # build .dev-app\Dipstick Dev.exe and (re)start it
npm run app:start  # start the last build
npm run app:stop   # quit it
npm run app:dev    # debug build with hot reload and a console window
```

`npm run app` is a full release build, so what you try is what would ship.
`npm run app:dev` rebuilds on every save and reloads the pages instantly —
quicker while working, slower to run.

What keeps the two apart:

|                  | Dipstick                                    | Dipstick Dev                                      |
| ---------------- | ------------------------------------------- | ------------------------------------------------- |
| Tray icon        | orange and lilac sticks, tooltip "Dipstick" | blue and olive sticks, tooltip "Dipstick Dev"     |
| Settings, cache  | `%APPDATA%\Dipstick`                        | `%APPDATA%\Dipstick Dev`                          |
| Identifier       | `io.github.qunqin24.DipstickWindows`        | `io.github.qunqin24.DipstickWindows.dev`          |
| Process          | `dipstick.exe`                              | `Dipstick Dev.exe` (`dipstick.exe` for `app:dev`) |
| Open at login    | set on first launch                         | never set on its own                              |
| Updates          | checks the feed, offers each version        | never — its Settings say so                       |
| Version in About | `0.1.10`                                    | `0.1.10-dev`                                      |

The separate identifier gives Dipstick Dev its own single-instance lock and
WebView2 profile, and a separate product name its own login-item entry. The
separate data folder comes from the `dev-copy` Cargo feature, which
`src-tauri/tauri.dev.conf.json` turns on. Every debug build uses that folder
too, whichever config it was built with.

Dipstick Dev asks the same endpoints with the same logins as Dipstick does, so
running both roughly doubles how often each service is asked. Claude accounts
added in Dipstick Dev are its own, in `%APPDATA%\Dipstick Dev\accounts`, but Claude
Code's own login is shared: **Use in Claude Code** in Dipstick Dev changes which
account Claude Code is signed in to, for the installed Dipstick and every Claude
Code window too. Dipstick writes no login otherwise; an expired one is renewed by
Claude Code itself.

Useful while testing:

```powershell
& ".dev-app\Dipstick Dev.exe" --json          # what Dipstick Dev last banked
Remove-Item -Recurse "$env:APPDATA\Dipstick Dev"  # start again from the chooser (quit it first)
```

### Renamed from Pulse

Dipstick was called Pulse until 0.1.10. The installed Pulse updates itself to
Dipstick, which Windows sees as a different app, so two things take over what
Pulse left behind:

- **The installer** (`src-tauri/installer-hooks.nsh`), after installing,
  copies Pulse's login entry, desktop shortcut and Start menu shortcut over to
  Dipstick. Then it runs Pulse's uninstaller silently and deletes Pulse's
  WebView2 folder.
- **Dipstick, on its first start**, moves `%APPDATA%\Pulse` to
  `%APPDATA%\Dipstick` with one rename, never a copy, since the accounts'
  logins are in it (`paths::data_dir`). If the folder can't be moved, it is
  used where it is and the next start tries again. Once it has moved, Pulse's
  login entry and its Task Manager on or off are carried over too
  (`renamed.rs`). A dev copy does the same with `%APPDATA%\Pulse Dev`.

A Pulse icon pinned to the taskbar is unpinned by Pulse's uninstaller, and
Windows doesn't let an app pin itself.

### What not to run on a PC with Dipstick or Pulse installed

- **`npm run tauri build`** without the dev config: its installer is the real
  Dipstick, and running it replaces the installed one, or removes Pulse. It
  also needs the updater's signing key, which only releases use — see
  [releasing.md](releasing.md).
- **`npm run tauri dev`** without the dev config: it has the installed app's
  identifier, so it hands over to the running Dipstick and exits.
- **`taskkill /IM dipstick.exe`**: that is the installed Dipstick. Use
  `npm run app:stop`.

## Known issues

Problems found and understood but deliberately left for later — with how to
reproduce them and a ready fix — are in [known-issues.md](known-issues.md).
