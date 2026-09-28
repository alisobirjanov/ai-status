# Development

Pulse for Windows is a [Tauri 2](https://tauri.app) app. Rust (`src-tauri/`)
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

## Trying a change: Pulse Dev

Most people working on Pulse also use it. A build of the checkout must not
move their rail, rewrite their settings or replace their install, so changes
are tried in **Pulse Dev**, a copy that runs beside the installed Pulse:

```powershell
npm run app        # build .dev-app\Pulse Dev.exe and (re)start it
npm run app:start  # start the last build
npm run app:stop   # quit it
npm run app:dev    # debug build with hot reload and a console window
```

`npm run app` is a full release build, so what you try is what would ship.
`npm run app:dev` rebuilds on every save and reloads the pages instantly —
quicker while working, slower to run.

What keeps the two apart:

|                  | Pulse                                   | Pulse Dev                                    |
| ---------------- | --------------------------------------- | -------------------------------------------- |
| Tray icon        | orange ring, tooltip "Pulse"            | blue ring, tooltip "Pulse Dev"               |
| Settings, cache  | `%APPDATA%\Pulse`                       | `%APPDATA%\Pulse Dev`                        |
| Identifier       | `io.github.qunqin24.PulseWindows`       | `io.github.qunqin24.PulseWindows.dev`        |
| Process          | `pulse.exe`                             | `Pulse Dev.exe` (`pulse.exe` for `app:dev`)  |
| Open at login    | set on first launch                     | never set on its own                         |
| Updates          | checks the feed, offers each version    | never — its Settings say so                  |
| Version in About | `0.1.1`                                 | `0.1.1-dev`                                  |

The separate identifier gives Pulse Dev its own single-instance lock and
WebView2 profile, and a separate product name its own login-item entry. The
separate data folder comes from the `dev-copy` Cargo feature, which
`src-tauri/tauri.dev.conf.json` turns on. Every debug build uses that folder
too, whichever config it was built with.

Pulse Dev asks the same endpoints with the same logins as Pulse does, so
running both roughly doubles how often each service is asked. Neither writes
to the login files.

Useful while testing:

```powershell
& ".dev-app\Pulse Dev.exe" --json          # what Pulse Dev last banked
Remove-Item -Recurse "$env:APPDATA\Pulse Dev"  # start again from the chooser (quit it first)
```

### What not to run on a PC with Pulse installed

- **`npm run tauri build`** without the dev config: its installer is the real
  Pulse, and running it replaces the installed one. It also needs the
  updater's signing key, which only releases use — see
  [releasing.md](releasing.md).
- **`npm run tauri dev`** without the dev config: it has the installed app's
  identifier, so it hands over to the running Pulse and exits.
- **`taskkill /IM pulse.exe`**: that is the installed Pulse. Use
  `npm run app:stop`.

## Known issues

Problems found and understood but deliberately left for later — with how to
reproduce them and a ready fix — are in [known-issues.md](known-issues.md).
