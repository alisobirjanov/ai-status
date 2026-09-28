// The settings window. Built once, then brought up to date from every
// snapshot in place, so a control somebody is using is never torn down under
// them by a refresh landing.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { reasonText, relative } from "../shared/format";
import { t, type StringKey } from "../shared/i18n";
import { icon } from "../shared/icons";
import type { AccountView, CodexSource, Provider, RingShows, SettingsPatch, Snapshot, UpdateInfo } from "../shared/types";
import "./settings.css";

const PROVIDERS: Provider[] = ["claudeCode", "codex"];
const INTERVALS = [0, 2, 5, 10, 15, 30];
const WARNING_CHOICES = [60, 70, 75, 80, 85, 90];

let snapshot: Snapshot | null = null;

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

function setText(id: string, key: StringKey) {
  document.getElementById(id)!.textContent = t(key);
}

function update(patch: SettingsPatch) {
  void invoke("update_settings", { patch });
}

function switchInput(label: string): HTMLInputElement {
  const input = element("input", "switch");
  input.type = "checkbox";
  input.setAttribute("role", "switch");
  input.setAttribute("aria-label", label);
  return input;
}

function select<T extends string | number>(label: string, options: [T, string][], onChange: (value: T) => void): HTMLSelectElement {
  const node = element("select");
  node.setAttribute("aria-label", label);
  for (const [value, text] of options) {
    const option = element("option", undefined, text);
    option.value = String(value);
    node.appendChild(option);
  }
  node.addEventListener("change", () => {
    const raw = node.value;
    const match = options.find(([value]) => String(value) === raw);
    if (match) onChange(match[0]);
  });
  return node;
}

// MARK: - Services

interface ServiceRow {
  toggle: HTMLInputElement;
  badge: HTMLElement;
  facts: HTMLDListElement;
  plan: HTMLElement;
  planLabel: HTMLElement;
  status: HTMLElement;
  refresh: HTMLButtonElement;
  route?: HTMLSelectElement;
  routeLine?: HTMLElement;
}

const rows = new Map<Provider, ServiceRow>();

function buildService(provider: Provider, name: string): HTMLElement {
  const box = element("div", "setting");

  const head = element("div", "line");
  const mark = element("span", "provider-icon");
  mark.appendChild(icon(provider, 16));
  const title = element("div", "grow");
  title.appendChild(element("span", "name", name));
  const badge = element("span", "badge");
  const toggle = switchInput(name);
  toggle.addEventListener("change", () => {
    const enabled = snapshot?.settings.enabled ?? [];
    update({ enabled: toggle.checked ? [...enabled, provider] : enabled.filter((p) => p !== provider) });
  });
  head.append(mark, title, badge, toggle);
  box.appendChild(head);

  box.appendChild(element("p", "hint", t(provider === "claudeCode" ? "accessClaude" : "accessCodex")));

  const facts = element("dl", "facts");
  const planLabel = element("dt", undefined, t("plan"));
  const plan = element("dd");
  const status = element("dd");
  facts.append(planLabel, plan, element("dt", undefined, t("status")), status);
  box.appendChild(facts);

  const actions = element("div", "line");
  actions.appendChild(element("div", "grow"));
  const row: ServiceRow = { toggle, badge, facts, plan, planLabel, status, refresh: element("button", undefined, t("refresh")) };

  if (provider === "codex") {
    const routeLine = element("div", "line");
    routeLine.appendChild(element("span", "grow", t("route")));
    row.route = select<CodexSource>(
      t("route"),
      [
        ["automatic", t("routeAutomatic")],
        ["endpoint", t("routeEndpoint")],
        ["tooling", t("routeTooling")],
      ],
      (value) => update({ codexSource: value }),
    );
    routeLine.appendChild(row.route);
    row.routeLine = routeLine;
    box.appendChild(routeLine);
  }

  row.refresh.addEventListener("click", () => void invoke("refresh", { provider }));
  actions.appendChild(row.refresh);
  box.appendChild(actions);
  rows.set(provider, row);
  return box;
}

function statusLine(account: AccountView): { text: string; problem: boolean } {
  if (!account.enabled) return { text: t("notShown"), problem: false };
  if (account.refreshing) return { text: t("checking"), problem: false };
  const usage = account.usage;
  const failed = account.lastCheck?.reason;
  if (usage.state === "live" && usage.observedAt != null) return { text: t("statusOk", relative(usage.observedAt)), problem: false };
  if (usage.state === "stale" && usage.observedAt != null) {
    const banked = t("statusFromBank", relative(usage.observedAt));
    return { text: failed ? `${banked}. ${reasonText(failed)}` : banked, problem: !!failed };
  }
  const reason = usage.reason ?? failed;
  return { text: reason ? reasonText(reason) : t("notChecked"), problem: !!reason && reason !== "notChecked" };
}

function updateServices(snap: Snapshot) {
  const settings = snap.settings;
  for (const account of snap.accounts) {
    const row = rows.get(account.provider);
    if (!row) continue;
    row.toggle.checked = account.enabled;
    // The rail is never empty once monitoring has started.
    const lastOne = settings.hasChosen && account.enabled && settings.enabled.length === 1;
    row.toggle.disabled = lastOne;
    row.toggle.title = lastOne ? t("lastOne") : "";

    row.badge.textContent = account.detected ? t("detected") : t("notDetected");
    row.badge.className = account.detected ? "badge" : "badge muted";

    row.facts.hidden = !settings.hasChosen;
    const plan = account.enabled ? account.usage.plan : null;
    row.plan.textContent = plan ?? "";
    row.plan.hidden = row.planLabel.hidden = !plan;

    const status = statusLine(account);
    row.status.textContent = status.text;
    row.status.className = status.problem ? "problem" : "";

    row.refresh.disabled = !account.enabled || account.refreshing;
    if (row.route && document.activeElement !== row.route) row.route.value = settings.codexSource;
    if (row.routeLine) row.routeLine.hidden = !account.enabled;
  }
}

// MARK: - Panel, refresh, general

const panelSwitch = switchInput(t("showPanel"));
const ringSelect = select<RingShows>(
  t("ringShows"),
  [
    ["fullest", t("ringFullest")],
    ["fiveHour", t("fiveHour")],
    ["weekly", t("weekly")],
    ["bothSplit", t("ringBothSplit")],
    ["bothStacked", t("ringBothStacked")],
    ["bothNested", t("ringBothNested")],
  ],
  (value) => update({ ringShows: value }),
);
const lettersSwitch = switchInput(t("limitLetters"));
const lettersLine = settingLine(t("limitLetters"), lettersSwitch, t("limitLettersHint"));
const remainingSwitch = switchInput(t("showsRemaining"));
const warningSelect = select<number>(
  t("warningAt"),
  WARNING_CHOICES.map((value) => [value, `${value}%`]),
  (value) => update({ warningAt: value }),
);
const intervalSelect = select<number>(
  t("interval"),
  INTERVALS.map((minutes) => [minutes, minutes === 0 ? t("intervalAutomatic") : t("intervalMinutes", String(minutes))]),
  (value) => update({ refreshMinutes: value }),
);
const loginSwitch = switchInput(t("launchAtLogin"));

function settingLine(label: string, control: HTMLElement, hint?: string): HTMLElement {
  const box = element("div", "setting");
  const line = element("div", "line");
  line.append(element("span", "grow", label), control);
  box.appendChild(line);
  if (hint) box.appendChild(element("p", "hint", hint));
  return box;
}

// MARK: - Updates

const updateName = element("div", "name");
const updateStatus = element("p", "hint");
const checkButton = element("button", undefined, t("checkNow"));
const installButton = element("button", "primary", t("installAndRestart"));
const updateProgress = element("div", "progress");
const updateProgressFill = element("div");
updateProgress.appendChild(updateProgressFill);
const notesTitle = element("div", "name");
const notes = element("p", "notes");
const autoUpdateSwitch = switchInput(t("autoUpdate"));
const autoUpdateLine = settingLine(t("autoUpdate"), autoUpdateSwitch, t("autoUpdateHint"));

function buildUpdates() {
  const box = element("div", "setting");
  const line = element("div", "line");
  const text = element("div", "grow");
  text.append(updateName, updateStatus);
  line.append(text, checkButton, installButton);
  box.append(line, updateProgress, notesTitle, notes);

  checkButton.addEventListener("click", () => void invoke("check_for_update"));
  // A failure comes back as an `update` event too, so there is nothing to
  // do with the rejection here.
  installButton.addEventListener("click", () => void invoke("install_update").catch(() => undefined));
  autoUpdateSwitch.addEventListener("change", () => update({ checksForUpdates: autoUpdateSwitch.checked }));

  document.getElementById("updates-group")!.append(box, autoUpdateLine);
}

function updateStatusText(info: UpdateInfo): string {
  const version = info.version ?? "";
  switch (info.status) {
    case "unsupported":
      return t("updateUnsupported");
    case "checking":
      return t("updateChecking");
    case "upToDate":
      return info.checkedAt != null ? t("updateUpToDate", relative(info.checkedAt)) : t("updateNotChecked");
    case "available":
      return t("updateAvailable", version);
    case "downloading": {
      const percent = info.progress != null ? ` ${Math.round(info.progress * 100)}%` : "";
      return t("updateDownloading", version) + percent;
    }
    case "installing":
      return t("updateInstalling");
    case "failed":
      return t("updateFailed", info.error ?? "");
    default:
      return t("updateNotChecked");
  }
}

function applyUpdate(info: UpdateInfo) {
  const busy = info.status === "checking" || info.status === "downloading" || info.status === "installing";
  const unsupported = info.status === "unsupported";

  updateName.textContent = t("updateVersion", info.current);
  updateStatus.textContent = updateStatusText(info);
  updateStatus.className = info.status === "failed" ? "hint problem" : "hint";

  checkButton.hidden = unsupported;
  checkButton.disabled = busy;
  // A version found and not yet installed stays on offer after a failed try.
  installButton.hidden = unsupported || info.version == null;
  installButton.disabled = busy;

  updateProgress.hidden = !(info.status === "downloading" || info.status === "installing");
  updateProgressFill.style.width = `${Math.round((info.progress ?? 0) * 100)}%`;

  const hasNotes = info.version != null && !!info.notes;
  notesTitle.hidden = notes.hidden = !hasNotes;
  notesTitle.textContent = hasNotes ? t("whatsNew", info.version!) : "";
  // The notes are the changelog's Markdown; its emphasis marks are noise here.
  notes.textContent = hasNotes ? info.notes!.replace(/\*\*/g, "") : "";

  autoUpdateLine.hidden = unsupported;
}

function build(snap: Snapshot) {
  document.title = t("settingsTitle");
  setText("chooser-title", "chooserTitle");
  setText("chooser-body", "chooserBody");
  setText("services-title", "services");
  setText("panel-title", "panel");
  setText("refresh-title", "refreshGroup");
  setText("general-title", "general");
  setText("updates-title", "updates");
  document.getElementById("about")!.textContent = t("about", snap.version);

  const services = document.getElementById("services")!;
  for (const provider of PROVIDERS) {
    const account = snap.accounts.find((a) => a.provider === provider);
    services.appendChild(buildService(provider, account?.name ?? provider));
  }

  panelSwitch.addEventListener("change", () => update({ panelVisible: panelSwitch.checked }));
  lettersSwitch.addEventListener("change", () => update({ limitLetters: lettersSwitch.checked }));
  remainingSwitch.addEventListener("change", () => update({ showsRemaining: remainingSwitch.checked }));
  document
    .getElementById("panel-group")!
    .append(
      settingLine(t("showPanel"), panelSwitch, t("showPanelHint")),
      settingLine(t("ringShows"), ringSelect, t("ringShowsHint")),
      lettersLine,
      settingLine(t("showsRemaining"), remainingSwitch, t("showsRemainingHint")),
      settingLine(t("warningAt"), warningSelect),
    );

  const refreshAll = element("button", undefined, t("refreshAll"));
  refreshAll.addEventListener("click", () => void invoke("refresh", { provider: null }));
  const intervalBox = settingLine(t("interval"), intervalSelect, t("intervalHint"));
  const actions = element("div", "line");
  actions.append(element("div", "grow"), refreshAll);
  intervalBox.appendChild(actions);
  document.getElementById("refresh-group")!.appendChild(intervalBox);

  loginSwitch.addEventListener("change", async () => {
    loginSwitch.checked = await invoke<boolean>("set_autostart", { enabled: loginSwitch.checked });
  });
  document.getElementById("general-group")!.appendChild(settingLine(t("launchAtLogin"), loginSwitch));
  void invoke<boolean>("get_autostart").then((on) => (loginSwitch.checked = on));

  buildUpdates();
}

function apply(snap: Snapshot) {
  snapshot = snap;
  const settings = snap.settings;
  document.getElementById("chooser")!.hidden = settings.hasChosen;
  for (const id of ["panel-title", "panel-group", "refresh-title", "refresh-group"]) {
    document.getElementById(id)!.hidden = !settings.hasChosen;
  }

  updateServices(snap);
  panelSwitch.checked = settings.panelVisible;
  remainingSwitch.checked = settings.showsRemaining;
  if (document.activeElement !== ringSelect) ringSelect.value = settings.ringShows;
  lettersSwitch.checked = settings.limitLetters;
  // Only a ring with both limits on it has two figures to tell apart.
  lettersLine.hidden = !settings.ringShows.startsWith("both");
  if (document.activeElement !== warningSelect) warningSelect.value = String(settings.warningAt);
  if (document.activeElement !== intervalSelect) intervalSelect.value = String(settings.refreshMinutes ?? 0);
  autoUpdateSwitch.checked = settings.checksForUpdates;
}

const first = await invoke<Snapshot>("get_snapshot");
build(first);
apply(first);
await listen<Snapshot>("snapshot", (event) => apply(event.payload));

let updateInfo = await invoke<UpdateInfo>("get_update");
applyUpdate(updateInfo);
await listen<UpdateInfo>("update", (event) => {
  updateInfo = event.payload;
  applyUpdate(updateInfo);
});

// "Updated 2 minutes ago" keeps up with the clock.
window.setInterval(() => {
  if (snapshot) updateServices(snapshot);
  applyUpdate(updateInfo);
}, 30_000);
