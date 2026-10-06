# Releasing

A release is a merge. When a push to `main` — a merged pull request, or a
push straight to it — carries a version that has no `v*` tag yet, GitHub
Actions (`.github/workflows/release.yml`) builds and signs the installer on a
Windows runner, publishes it as this repository's latest release — which is
exactly what installed copies ask — and makes the tag on that commit.

## Cutting a release

In the pull request (or on `main`):

```powershell
npm run release:version 0.2.0   # writes the version everywhere it lives
# write the "## 0.2.0" entry in CHANGELOG.md — Russian, then English
npm run release:check 0.2.0     # what the workflow will check first
git commit -am "Dipstick 0.2.0"
```

Merge it, and 0.2.0 is released. No tag to push.

The pull request already says what merging will do: its **Build** check
fails if the new version has no `CHANGELOG.md` entry or disagrees with a
file, and its summary says "Merging this into main releases Dipstick 0.2.0" —
or, for a version already released, that merging publishes nothing.

**A merge that does not raise the version releases nothing.** An installed
copy only takes a version newer than its own, so the same version cannot be
offered twice; the run says so in its summary and succeeds.

On `main`, the workflow then:

1. refuses a version that disagrees with `package.json`, `tauri.conf.json`,
   `Cargo.toml` or `Cargo.lock`, or has no `CHANGELOG.md` entry — before
   anything is built;
2. builds the pages and runs the tests (warnings are failures);
3. builds the NSIS installer and signs it for the updater;
4. writes `latest.json` — the version, the changelog entry, the installer's
   address and its signature;
5. publishes `Dipstick-<version>-x64-setup.exe`, its `.sig` and `latest.json` as
   release `v<version>`: as a draft until every file is there, then as the
   latest release, making tag `v<version>` on the commit it built;
6. asks the feed the way an installed copy does, and fails if it does not
   offer the new version.

Because the tag is made only when the release is published, a run that fails
before then leaves nothing behind: fix the cause, and the next push to `main`
tries the same version again. Two merges close together are released one
after the other; the second finds the first's tag.

A version with a suffix (`0.2.0-beta.1`) is published as a pre-release.
GitHub never makes a pre-release the latest release, so installed copies are
not offered it — only people who download it get it.

A `v*` tag pushed by hand still publishes that tag (`git tag v0.2.0` and
`git push origin v0.2.0`), for a commit that is not on `main`'s tip. Do not
also merge the same version: the merge's run would find the tag and do
nothing, but a hand-pushed tag and a merge racing each other both build.

If a step fails after publishing, fix the cause and re-run the workflow for
the same tag (**Actions → Release → Run workflow**, tag `v0.2.0`). It replaces
the assets rather than failing on the release that already exists.

`.github/workflows/ci.yml` runs the same pages build and tests on every push
to `main` and every pull request, without building an installer.

## How installed copies update

`src-tauri/src/updater.rs`, with `tauri-plugin-updater`.

- A minute after launch, and then every six hours by the wall clock, Dipstick
  asks `https://github.com/alisobirjanov/ai-status/releases/latest/download/latest.json`.
  A PC that slept through the interval asks when it wakes. **Settings →
  Updates** can turn the automatic check off; **Check Now** always works.
- A newer version is announced **once** with a Windows notification
  (`updateAnnounced` in `settings.json` remembers which), and stays on offer
  as **Install Update x.y.z…** at the top of the tray menu and in Settings,
  with the changelog entry under it.
- **Nothing installs by itself.** Installing replaces the running app, so it
  happens only when somebody chooses it. Dipstick downloads the installer
  (progress in Settings), checks its signature, and hands over to it; the
  installer runs in passive mode — a progress bar, no questions — and starts
  Dipstick again when it is done.
- Before the first release there is no feed, and the automatic check fails
  quietly. Only **Check Now** reports a failure.
- **Pulse updates to Dipstick the same way.** Dipstick was Pulse until
  0.1.10, and the feed's address didn't change. To Windows, Dipstick is a
  new app: its installer removes Pulse, and Dipstick moves Pulse's settings
  and accounts on its first start. See *Renamed from Pulse* in
  [development.md](development.md).
- **Dipstick Dev never updates.** The feed's installer is the real Dipstick, and
  installing it from a dev copy would replace somebody's installed Dipstick. Its
  Settings say so instead of offering a check.

## The signing key

The updater runs an installer only if its signature verifies against the
public key in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`), and —
with `requireSignedVersion` — only if the version inside the signature is the
one `latest.json` announces, so an edited feed cannot pass an old installer
off as a new version.

The private half signs every release in CI and is kept in two repository
secrets (**Settings → Secrets and variables → Actions**):

| Secret                               | Value                                |
| ------------------------------------ | ------------------------------------ |
| `TAURI_SIGNING_PRIVATE_KEY`          | the contents of the private key file |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password                         |

With the GitHub CLI:

```powershell
gh secret set TAURI_SIGNING_PRIVATE_KEY < $HOME\.tauri\pulse-windows.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD < $HOME\.tauri\pulse-windows.key.password
```

**Keep a copy of the private key and its password somewhere safe.** Losing
them means no installed copy can be updated again: a new key pair needs a new
public key in the app, which only a manual reinstall delivers. Never commit
the private key.

To build a signed installer locally (it is the real Dipstick — build it, don't
run it on a PC where Dipstick or Pulse is installed):

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw $HOME\.tauri\pulse-windows.key
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content -Raw $HOME\.tauri\pulse-windows.key.password
npx tauri build
```

Without the key `tauri build` stops at the updater signature. `npm run app`
(Dipstick Dev) never needs it.

## Not done yet

- **The installer is not Authenticode-signed**, so SmartScreen warns on the
  first download ("Windows protected your PC" → *More info* → *Run anyway*).
  Updates are not affected: they are verified by the updater's own signature.
