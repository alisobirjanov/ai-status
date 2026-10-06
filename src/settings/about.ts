// About: which Dipstick this is and whether a newer one is out, what is new in
// it, and everything Dipstick touches.

import { invoke } from "@tauri-apps/api/core";
import {
  ArrowUpRight,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CodeXml,
  Download,
  FileLock,
  FolderOpen,
  Globe,
  LoaderCircle,
  MessageCircleWarning,
  RefreshCw,
  ShieldOff,
  Trash2,
  type IconNode,
} from "lucide";

import { locale, t } from "../shared/i18n";
import type { SettingsPatch, Snapshot, UpdateInfo } from "../shared/types";
import { entry, points } from "./changelog";
import { brandMark, button, el, fillIn, icon, reveal, row, sectionHead, segmented, soonBadge, stageHead, toggle } from "./dom";

/** Where the page may send somebody: `Link` in `lib.rs`. */
type Link = "issues" | "source" | "changelog" | "dataFolder";

function open(link: Link) {
  void invoke("open_link", { link });
}

/** "today at 09:12", or the date for another day. */
function when(ms: number): string {
  const date = new Date(ms);
  if (date.toDateString() === new Date().toDateString()) {
    return t("todayAt", new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" }).format(date));
  }
  return new Intl.DateTimeFormat(locale, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }).format(date);
}

function list(items: string[], className = "privacy-list"): HTMLUListElement {
  return el("ul", className, ...items.map((item) => el("li", undefined, item)));
}

export interface About {
  element: HTMLElement;
  apply(snapshot: Snapshot): void;
  applyUpdate(info: UpdateInfo): void;
  /** Fills the mark in each time the page comes up. */
  shown(): void;
  tick(): void;
}

export function about(update: (patch: SettingsPatch) => void): About {
  // The stage

  const logo = brandMark(116);
  const version = el("p", "about-version");
  const pill = el("p", "status-pill");
  const actionLabel = el("span");
  const action = button("primary-button", actionLabel);
  let installs = false;
  action.addEventListener("click", () => {
    // A failure comes back as an `update` event too.
    if (installs) void invoke("install_update").catch(() => undefined);
    else void invoke("check_for_update");
  });
  const progressFill = el("div", "progress-fill");
  const progressText = el("span", "progress-text");
  const progress = el("div", "progress-row", el("div", "progress", progressFill), progressText);
  const lastChecked = el("p", "last-checked");
  const detail = el("p", "update-detail");

  const stage = el(
    "aside",
    "stage about-stage",
    stageHead("Dipstick", t("forWindows")),
    el("div", "about-hero", el("div", "logo-halo", logo), el("h1", "about-name", "Dipstick"), version, pill),
    el("div", "about-actions", action, progress, lastChecked, detail),
    el("p", "disclaimer", t("disclaimer")),
  );

  function setPill(tone: "good" | "accent" | "muted" | "danger", glyph: IconNode, text: string, spinning = false) {
    pill.className = `status-pill ${tone}`;
    const mark = icon(glyph, 14);
    if (spinning) mark.classList.add("spin");
    pill.replaceChildren(mark, el("span", undefined, text));
  }

  function setAction(glyph: IconNode, text: string, disabled: boolean, spinning: boolean) {
    const mark = icon(glyph, 16);
    if (spinning) mark.classList.add("spin");
    actionLabel.textContent = text;
    action.replaceChildren(mark, actionLabel);
    action.disabled = disabled;
  }

  // Updates

  const autoUpdate = toggle(t("autoUpdate"), (on) => update({ checksForUpdates: on }));
  const channel = segmented<string>(t("channel"), [
    ["stable", t("channelStable")],
    ["beta", t("channelBeta")],
  ]);
  channel.set("stable");
  channel.disable(true);
  const installAuto = toggle(t("installAuto"));
  installAuto.disable(true);
  const soonRow = (title: string, hint: string, control: HTMLElement) => {
    const line = row(el("span", "with-badge", title, soonBadge()), hint, control);
    line.classList.add("soon");
    return line;
  };
  const updates = el(
    "section",
    "section",
    sectionHead(t("updates"), t("updatesNote")),
    el(
      "article",
      "card rows",
      row(t("autoUpdate"), t("autoUpdateHint"), autoUpdate.element),
      soonRow(t("channel"), t("channelHint"), channel.element),
      soonRow(t("installAuto"), t("installAutoHint"), installAuto.element),
    ),
  );

  // What's new

  const notesTitle = el("h3");
  const notesTag = el("span", "tag");
  const notesList = el("ul", "notes");
  const changelog = button("link-button", el("span", undefined, t("fullChangelog")), icon(ArrowUpRight, 14));
  changelog.addEventListener("click", () => open("changelog"));
  const whatsNew = el(
    "article",
    "card whats-new",
    el("div", "whats-new-head", el("div", "whats-new-title", notesTitle, notesTag), changelog),
    notesList,
  );
  let notesFor = "";

  function applyNotes(info: UpdateInfo) {
    // A version on offer, with its notes; otherwise the one running, from
    // the changelog it was built with.
    const offered = info.version != null && !!info.notes;
    const shown = offered ? info.version! : info.current.replace(/-dev$/, "");
    const key = `${shown}:${offered}`;
    if (key === notesFor) return;
    notesFor = key;
    const body = offered ? info.notes! : entry(shown);
    const lines = body ? points(body) : [];
    notesTitle.textContent = t("whatsNew", shown);
    notesTag.textContent = offered ? t("tagNew") : t("tagInstalled");
    notesTag.classList.toggle("new", offered);
    notesList.replaceChildren(
      ...(lines.length
        ? lines.map((line, index) => el("li", undefined, el("span", `note-dot n${index % 3}`), el("span", undefined, line)))
        : [el("li", "muted", t("noNotes"))]),
    );
  }

  // Privacy and help

  const reads = list([], "privacy-list mono");
  const tile = (glyph: IconNode, tone: string, title: string, items: HTMLElement) =>
    el("article", `card privacy-tile ${tone}`, el("h3", undefined, icon(glyph, 18), el("span", undefined, title)), items);
  const privacy = el(
    "section",
    "section",
    sectionHead(t("privacy"), t("privacyNote")),
    el(
      "div",
      "privacy-tiles",
      tile(FileLock, "five", t("readsOnly"), reads),
      tile(Globe, "week", t("talksTo"), list([t("endpointAnthropic"), t("endpointOpenAI"), t("endpointGitHub"), t("claudeCodeLocal")])),
      tile(ShieldOff, "good", t("never"), list([t("neverAnalytics"), t("neverLogins")])),
    ),
  );

  const linkButton = (glyph: IconNode, text: string, link: Link) => {
    const control = button("button", icon(glyph, 15), el("span", undefined, text));
    control.addEventListener("click", () => open(link));
    return control;
  };
  const resetLabel = el("span", undefined, t("resetAll"));
  const reset = button("button danger", icon(Trash2, 15), resetLabel);
  reset.title = t("resetHint");
  let armed: number | undefined;
  function disarm() {
    window.clearTimeout(armed);
    armed = undefined;
    reset.classList.remove("armed");
    resetLabel.textContent = t("resetAll");
  }
  // Two clicks: one to ask, one to mean it.
  reset.addEventListener("click", () => {
    if (armed === undefined) {
      reset.classList.add("armed");
      resetLabel.textContent = t("resetConfirm");
      armed = window.setTimeout(disarm, 4000);
      return;
    }
    disarm();
    void invoke("reset_settings");
  });
  reset.addEventListener("blur", disarm);
  const help = el(
    "div",
    "help-row",
    linkButton(MessageCircleWarning, t("reportIssue"), "issues"),
    linkButton(FolderOpen, t("openDataFolder"), "dataFolder"),
    linkButton(CodeXml, t("source"), "source"),
    reset,
  );

  const controls = el("div", "controls", updates, whatsNew, privacy, help);
  [updates, whatsNew, privacy, help].forEach((part, index) => reveal(part, index + 1));
  const element = el("section", "page", reveal(stage, 0), controls);

  let info: UpdateInfo | null = null;

  function applyUpdate(next: UpdateInfo) {
    info = next;
    const busy = next.status === "checking" || next.status === "downloading" || next.status === "installing";
    const unsupported = next.status === "unsupported";
    const offered = next.version ?? "";
    version.textContent = t("versionLine", next.current);

    let hint: string | null = null;
    switch (next.status) {
      case "unsupported":
        setPill("muted", CircleDashed, t("updateUnsupported"));
        hint = t("updateUnsupportedHint");
        break;
      case "checking":
        setPill("muted", LoaderCircle, t("updateChecking"), true);
        break;
      case "upToDate":
        setPill("good", CircleCheck, t("upToDate"));
        break;
      case "available":
        setPill("accent", Download, t("updateAvailable", offered));
        break;
      case "downloading":
        setPill("accent", Download, t("updateDownloading", offered));
        break;
      case "installing":
        setPill("accent", LoaderCircle, t("updateInstalling"), true);
        hint = t("updateInstallingHint");
        break;
      case "failed":
        setPill("danger", CircleAlert, t("updateFailed"));
        hint = next.error;
        break;
      default:
        setPill("muted", CircleDashed, t("updateNotChecked"));
    }

    // A version found and not yet installed stays on offer after a failed try.
    installs = next.version != null && !unsupported;
    setAction(
      installs ? Download : RefreshCw,
      installs ? t("installAndRestart") : t("checkForUpdates"),
      busy || unsupported,
      next.status === "checking",
    );

    const moving = next.status === "downloading" || next.status === "installing";
    progress.hidden = !moving;
    const fraction = Math.min(Math.max(next.progress ?? 0, 0), 1);
    progressFill.style.transform = `translateX(${(fraction - 1) * 100}%)`;
    progressText.textContent = `${Math.round(fraction * 100)}%`;

    lastChecked.hidden = next.checkedAt == null;
    lastChecked.textContent = next.checkedAt != null ? t("lastChecked", when(next.checkedAt)) : "";
    detail.hidden = !hint;
    detail.textContent = hint ?? "";
    detail.classList.toggle("problem", next.status === "failed");

    autoUpdate.disable(unsupported);
    applyNotes(next);
  }

  return {
    element,
    apply(snapshot) {
      autoUpdate.set(snapshot.settings.checksForUpdates);
      reads.replaceChildren(...snapshot.accounts.map((account) => el("li", undefined, account.credentials)));
    },
    applyUpdate,
    shown() {
      logo.querySelectorAll<SVGRectElement>(".stick-fill").forEach((fill, index) => fillIn(fill, 120 + index * 140, 900));
    },
    tick() {
      if (info) applyUpdate(info);
    },
  };
}
