// "Pulse Dev": a release build of this checkout that runs beside the
// installed Pulse without touching it — its own identifier, data folder,
// tray icon and executable name. See docs/development.md.
//
//   node scripts/dev-app.mjs         build it, then (re)start it
//   node scripts/dev-app.mjs start   start the last build
//   node scripts/dev-app.mjs stop    quit it

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const built = join(root, "src-tauri", "target", "release", "pulse.exe");
const folder = join(root, ".dev-app");
// Its own image name, so stopping it can never stop the installed pulse.exe.
const image = "Pulse Dev.exe";
const exe = join(folder, image);

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function stop() {
  // Forced: a Pulse with its tray icon up outlives a polite close. Nothing
  // is lost — settings are written the moment they change.
  const result = spawnSync("taskkill", ["/IM", image, "/T", "/F"], { stdio: "ignore" });
  return result.status === 0;
}

function start() {
  if (!existsSync(exe)) {
    console.error(`No ${image} yet. Build it with: npm run app`);
    process.exit(1);
  }
  // A second launch while one runs only brings up its Settings.
  spawn(exe, [], { detached: true, stdio: "ignore" }).unref();
  console.log(`Started ${exe}`);
}

function build() {
  // One string: `npx` is a .cmd on Windows, which only a shell can start.
  const cli = spawnSync("npx tauri build --no-bundle --config src-tauri/tauri.dev.conf.json", {
    cwd: root,
    stdio: "inherit",
    shell: true,
  });
  if (cli.status !== 0) process.exit(cli.status ?? 1);

  // The running copy keeps going through the compile; it is only replaced here.
  if (stop()) sleep(500);
  mkdirSync(folder, { recursive: true });
  for (let attempt = 1; ; attempt++) {
    try {
      copyFileSync(built, exe);
      break;
    } catch (error) {
      // The old process can hold the file for a moment after it is gone.
      if (attempt === 10) throw error;
      sleep(300);
    }
  }
  console.log(`Built ${exe}`);
}

switch (process.argv[2]) {
  case undefined:
    build();
    start();
    break;
  case "start":
    start();
    break;
  case "stop":
    console.log(stop() ? `Stopped ${image}` : `${image} was not running`);
    break;
  default:
    console.error("Usage: node scripts/dev-app.mjs [start|stop]");
    process.exit(1);
}
