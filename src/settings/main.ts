// The settings window: a bar with four pages under it. General and About are
// built; Alerts and Shortcuts show what is coming, and say so. Everything is
// built once and brought up to date from every snapshot in place, so a
// control somebody is using is never torn down under them by a refresh.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Moon, Sun } from "lucide";

import { relative } from "../shared/format";
import { locale, t, type StringKey } from "../shared/i18n";
import { pageScheme } from "../shared/theme";
import type { SettingsPatch, Snapshot, UpdateInfo } from "../shared/types";
import { about } from "./about";
import { arrowKeys, brandMark, button, check, el, icon, reducedMotion, slidingThumb, soonBadge } from "./dom";
import { general } from "./general";
import { alertsPage, shortcutsPage } from "./soon";
import "@fontsource-variable/manrope";
import "@fontsource-variable/bricolage-grotesque";
import "@fontsource-variable/jetbrains-mono";
import "../shared/rail.css";
import "./settings.css";

document.title = t("settingsTitle");
document.documentElement.lang = locale;

let snapshot = await invoke<Snapshot>("get_snapshot");

// MARK: - Changing a setting

/**
 * Settings go to Rust, which answers with a snapshot. Meanwhile the page
 * draws what was asked for at once — except which services are on, which
 * only Rust can say — and holds any snapshot that lands before every change
 * has been answered, so a quick run of clicks never flickers back.
 */
let pending = 0;
let held: Snapshot | null = null;

function update(patch: SettingsPatch) {
  if (patch.enabled === undefined) {
    const settings = { ...snapshot.settings, ...patch };
    if (patch.refreshMinutes !== undefined) settings.refreshMinutes = patch.refreshMinutes > 0 ? patch.refreshMinutes : null;
    snapshot = { ...snapshot, settings };
    render();
  }
  pending++;
  void invoke("update_settings", { patch }).finally(() => {
    pending--;
    if (pending === 0 && held) {
      const next = held;
      held = null;
      receive(next);
    }
  });
}

function receive(next: Snapshot) {
  if (pending > 0) {
    held = next;
    return;
  }
  snapshot = next;
  render();
}

// MARK: - The bar

const generalPage = general(update);
const aboutPage = about(update);

interface Tab {
  label: StringKey;
  element: HTMLElement;
  soon?: boolean;
  shown?: () => void;
}

const pages: Tab[] = [
  { label: "tabGeneral", element: generalPage.element },
  { label: "tabAlerts", element: alertsPage(), soon: true },
  { label: "tabShortcuts", element: shortcutsPage(), soon: true },
  { label: "tabAbout", element: aboutPage.element, shown: aboutPage.shown },
];
const ABOUT = 3;

const tablist = el("nav", "tabs");
tablist.setAttribute("role", "tablist");
const thumb = el("span", "tab-thumb");
tablist.append(thumb);
const aboutDot = el("span", "tab-dot");
aboutDot.hidden = true;

const tabs = pages.map((page, index) => {
  // Measured at its boldest, so choosing a tab never nudges the others.
  const label = el("span", "tab-label", t(page.label));
  label.dataset.label = t(page.label);
  const tab = button("tab", label, page.soon ? soonBadge() : null, index === ABOUT ? aboutDot : null);
  tab.setAttribute("role", "tab");
  tab.id = `tab-${index}`;
  page.element.id = `page-${index}`;
  page.element.setAttribute("role", "tabpanel");
  page.element.setAttribute("aria-labelledby", tab.id);
  tab.setAttribute("aria-controls", page.element.id);
  tab.addEventListener("click", () => show(index));
  tablist.append(tab);
  return tab;
});
arrowKeys(tablist, () => tabs);

const devTag = el("span", "dev-tag", "Dev");
const syncText = el("span", "sync-text");
const sync = el("div", "sync", el("span", "sync-dot"), syncText);
sync.setAttribute("aria-live", "polite");
// Light or dark in a click, whichever the page isn't in now.
const themeSwitch = button("theme-switch");
themeSwitch.addEventListener("click", () => update({ theme: pageScheme(snapshot) === "dark" ? "light" : "dark" }));
document
  .querySelector("header")!
  .append(
    el("div", "brand", brandMark(18, 3.6, 4), el("span", "brand-name", "Pulse"), devTag),
    tablist,
    el("div", "bar-end", sync, el("span", "bar-divider"), themeSwitch),
  );

const main = document.querySelector("main")!;
main.append(...pages.map((page) => page.element));

let current = -1;
const placeThumb = slidingThumb(tablist, thumb, () => tabs[current]);

/** Pages slide in from the side of the tab they were reached from. */
function show(index: number) {
  if (index === current) return;
  const direction = current < 0 ? 0 : Math.sign(index - current);
  current = index;
  check(tabs, tabs[index], "aria-selected");
  placeThumb();
  pages.forEach((page, i) => (page.element.hidden = i !== index));
  main.scrollTop = 0;
  const element = pages[index].element;
  element.style.setProperty("--dx", `${direction * 28}px`);
  element.classList.remove("entering");
  void element.offsetWidth;
  element.classList.add("entering");
  pages[index].shown?.();
}

// MARK: - From Rust

/** When the figures last came in, and whether all of them are live. */
function renderSync() {
  const on = snapshot.settings.hasChosen ? snapshot.accounts.filter((account) => account.enabled) : [];
  let tone: string;
  let text: string;
  if (on.length === 0) {
    tone = "off";
    text = t("syncOff");
  } else if (on.some((account) => account.refreshing)) {
    tone = "busy";
    text = t("checking");
  } else {
    const latest = Math.max(...on.map((account) => account.usage.observedAt ?? 0));
    text = latest > 0 ? t("synced", relative(latest)) : t("syncNever");
    tone = on.every((account) => account.usage.state === "live") ? "good" : "stale";
  }
  sync.dataset.tone = tone;
  syncText.textContent = text;
  sync.title = text;
}

/**
 * The theme the page is in, and the switch showing it: a moon while dark, a
 * sun while light. A change cross-fades the whole window rather than letting
 * each part change at its own pace.
 */
function applyTheme() {
  const root = document.documentElement;
  const scheme = pageScheme(snapshot);
  if (root.dataset.theme === scheme) return;
  const first = root.dataset.theme === undefined;
  const change = () => {
    root.dataset.theme = scheme;
    themeSwitch.classList.toggle("turned", !first);
    themeSwitch.replaceChildren(icon(scheme === "dark" ? Moon : Sun, 14));
    const label = t(scheme === "dark" ? "toLightTheme" : "toDarkTheme");
    themeSwitch.setAttribute("aria-label", label);
    themeSwitch.title = label;
  };
  if (first || reducedMotion.matches) change();
  else document.startViewTransition(change);
}

function render() {
  applyTheme();
  devTag.hidden = !snapshot.version.endsWith("-dev");
  generalPage.apply(snapshot);
  aboutPage.apply(snapshot);
  renderSync();
}

render();

let updateInfo = await invoke<UpdateInfo>("get_update");
function applyUpdate() {
  aboutPage.applyUpdate(updateInfo);
  aboutDot.hidden = updateInfo.version == null || updateInfo.status === "unsupported";
}
applyUpdate();

// The tray's "Install Update" opens Settings on the download.
const installing = updateInfo.status === "downloading" || updateInfo.status === "installing";
show(installing ? ABOUT : 0);

await listen<Snapshot>("snapshot", (event) => receive(event.payload));
await listen<UpdateInfo>("update", (event) => {
  const was = updateInfo.status;
  updateInfo = event.payload;
  applyUpdate();
  if (updateInfo.status === "downloading" && was !== "downloading") show(ABOUT);
});

// "Synced 2 minutes ago" and "resets in 1h 12m" keep up with the clock.
window.setInterval(() => {
  generalPage.tick();
  aboutPage.tick();
  renderSync();
}, 30_000);

// No browser menu: this is a window, not a web page.
document.addEventListener("contextmenu", (event) => event.preventDefault());
