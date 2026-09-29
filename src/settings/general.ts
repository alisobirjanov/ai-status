// General: the live preview beside what it previews — the services Pulse
// reads, how the rail looks, when Pulse looks, and the theme it is all in.

import { invoke } from "@tauri-apps/api/core";
import {
  ArrowRightToLine,
  ChevronDown,
  CircleAlert,
  Hourglass,
  Info,
  Monitor,
  Moon,
  PanelRight,
  RotateCw,
  Sparkles,
  SquareMousePointer,
  Sun,
  Type,
  type IconNode,
} from "lucide";

import { reasonText, relative } from "../shared/format";
import { icon as productMark } from "../shared/icons";
import { t, type StringKey } from "../shared/i18n";
import { svg } from "../shared/rail";
import type { AccountView, CodexSource, Provider, RingShows, Scheme, SettingsPatch, Snapshot, Theme } from "../shared/types";
import {
  arc,
  arrowKeys,
  button,
  check,
  drawIn,
  el,
  heading,
  icon,
  reveal,
  row,
  sectionHead,
  segmented,
  toggle,
  toggleTile,
  track,
  type Switch,
} from "./dom";
import { preview } from "./preview";

const PROVIDERS: Provider[] = ["claudeCode", "codex"];
const INTERVALS = [0, 2, 5, 10, 15, 30];
/** `WARNING_CHOICES` in `settings.rs`. */
const WARNING_CHOICES = [60, 70, 75, 80, 85, 90];

const RING_CHOICES: [RingShows, StringKey][] = [
  ["fullest", "ringFullest"],
  ["fiveHour", "ringFiveHour"],
  ["weekly", "ringWeekly"],
  ["bothSplit", "ringBothSplit"],
  ["bothStacked", "ringBothStacked"],
  ["bothNested", "ringBothNested"],
];

const THEME_CHOICES: [Theme, StringKey, IconNode][] = [
  ["system", "themeSystem", Monitor],
  ["light", "themeLight", Sun],
  ["dark", "themeDark", Moon],
];

// MARK: - Services

interface ServiceCard {
  element: HTMLElement;
  toggle: Switch;
  /** Says why the last service on cannot go off, after somebody tries. */
  note: HTMLElement;
  refused(): void;
  found: HTMLElement;
  path: HTMLElement;
  problem: HTMLElement;
  problemText: HTMLElement;
  plan: HTMLElement;
  planValue: HTMLElement;
  checked: HTMLElement;
  checkedValue: HTMLElement;
  refresh: HTMLButtonElement;
  route?: HTMLSelectElement;
}

function meta(label: string, value: HTMLElement): HTMLElement {
  return el("div", "meta", el("span", "meta-label", label), value);
}

/** Why a service's figures are old or missing, if they are. */
function problem(account: AccountView): string | null {
  if (!account.enabled || account.refreshing) return null;
  const usage = account.usage;
  const failed = account.lastCheck?.reason;
  if (usage.state === "stale" && usage.observedAt != null) {
    const banked = t("statusFromBank", relative(usage.observedAt));
    return failed && failed !== "notChecked" ? `${banked}. ${reasonText(failed)}` : banked;
  }
  const reason = usage.state === "unavailable" ? (usage.reason ?? failed) : null;
  return reason && reason !== "notChecked" ? reasonText(reason) : null;
}

function checkedText(account: AccountView): string {
  if (account.refreshing) return t("checking");
  const at = account.lastCheck?.checkedAt ?? account.usage.observedAt;
  return at != null ? relative(at, "short") : t("checkedNever");
}

export interface Page {
  element: HTMLElement;
  apply(snapshot: Snapshot): void;
  tick(): void;
}

export function general(update: (patch: SettingsPatch) => void): Page {
  let snapshot: Snapshot | null = null;
  const stage = preview();

  // Services

  const chooser = el(
    "div",
    "callout",
    el("span", "callout-icon", icon(Sparkles, 18)),
    heading(t("chooserTitle"), t("chooserBody")),
  );

  const cards = new Map<Provider, ServiceCard>();
  function serviceCard(provider: Provider): ServiceCard {
    const account = snapshot?.accounts.find((a) => a.provider === provider);
    const name = account?.name ?? provider;
    const control = toggle(name, (on) => {
      const enabled = snapshot?.settings.enabled ?? [];
      update({ enabled: on ? [...enabled, provider] : enabled.filter((p) => p !== provider) });
    });
    const found = el("p", "found", el("span", "dot"), el("span"));
    const path = el("code", "path");
    const note = el("p", "note", icon(Info, 14), el("span", undefined, t("lastOne")));
    note.hidden = true;
    let noteTimer: number | undefined;
    const refused = () => {
      note.hidden = false;
      window.clearTimeout(noteTimer);
      noteTimer = window.setTimeout(() => (note.hidden = true), 6000);
    };
    const problemText = el("span");
    const problemLine = el("p", "problem", icon(CircleAlert, 14), problemText);
    const planValue = el("span", "meta-value");
    const checkedValue = el("span", "meta-value");
    const plan = meta(t("plan"), planValue);
    const checked = meta(t("checked"), checkedValue);
    const refresh = button("icon-button", icon(RotateCw, 15));
    refresh.setAttribute("aria-label", `${t("refresh")} ${name}`);
    refresh.title = t("refresh");
    refresh.addEventListener("click", () => void invoke("refresh", { provider }));

    // The figures wrap among themselves; the refresh button keeps its corner.
    const metas = el("div", "metas", plan, checked);
    let route: HTMLSelectElement | undefined;
    if (provider === "codex") {
      route = el("select", "select");
      route.setAttribute("aria-label", t("route"));
      for (const [value, key] of [
        ["automatic", "routeAutomatic"],
        ["endpoint", "routeEndpoint"],
        ["tooling", "routeTooling"],
      ] as [CodexSource, StringKey][]) {
        const option = el("option", undefined, t(key));
        option.value = value;
        route.append(option);
      }
      const chosen = route;
      chosen.addEventListener("change", () => update({ codexSource: chosen.value as CodexSource }));
      metas.append(meta(t("route"), el("span", "select-wrap", route, icon(ChevronDown, 14))));
    }
    const foot = el("div", "service-foot", metas, refresh);

    const element = el(
      "article",
      "card service",
      el(
        "div",
        "service-head",
        el("span", "glyph", productMark(provider, 18)),
        el("div", "service-title", el("h3", undefined, name), found),
        control.element,
      ),
      path,
      el("p", "desc", t(provider === "claudeCode" ? "accessClaude" : "accessCodex")),
      note,
      problemLine,
      foot,
    );
    return { element, toggle: control, note, refused, found, path, problem: problemLine, problemText, plan, planValue, checked, checkedValue, refresh, route };
  }

  const servicesGrid = el("div", "services");

  function applyService(card: ServiceCard, account: AccountView, snap: Snapshot) {
    const settings = snap.settings;
    card.element.classList.toggle("on", account.enabled);
    card.toggle.set(account.enabled);
    // The rail is never empty once monitoring has started.
    const lastOne = settings.hasChosen && account.enabled && settings.enabled.length === 1;
    card.toggle.hold(lastOne, t("lastOne"), card.refused);
    if (!lastOne) card.note.hidden = true;
    card.found.classList.toggle("missing", !account.detected);
    card.found.lastElementChild!.textContent = account.detected ? t("detected") : t("notDetected");
    card.path.textContent = account.credentials;
    card.path.title = account.credentials;

    card.plan.hidden = card.checked.hidden = !account.enabled;
    card.planValue.textContent = account.usage.plan ?? "–";
    card.checkedValue.textContent = checkedText(account);
    card.refresh.disabled = !account.enabled || account.refreshing;
    card.refresh.classList.toggle("spinning", account.refreshing);

    const why = problem(account);
    card.problem.hidden = !why;
    card.problemText.textContent = why ?? "";
    if (card.route && document.activeElement !== card.route) card.route.value = settings.codexSource;
  }

  const services = el("section", "section", sectionHead(t("services"), t("servicesNote")), chooser, servicesGrid);

  // The panel

  const ringButtons = new Map<RingShows, HTMLButtonElement>();
  const ringRow = el("div", "ring-options");
  ringRow.setAttribute("role", "radiogroup");
  ringRow.setAttribute("aria-label", t("ringShows"));
  for (const [shows, key] of RING_CHOICES) {
    const option = button("ring-option", ringGlyph(shows), el("span", undefined, t(key)));
    option.setAttribute("role", "radio");
    option.addEventListener("click", () => {
      if (snapshot?.settings.ringShows !== shows) update({ ringShows: shows });
    });
    ringButtons.set(shows, option);
    ringRow.append(option);
  }
  arrowKeys(ringRow, () => [...ringButtons.values()]);
  let shownRing: RingShows | null = null;

  const panelTile = toggleTile(PanelRight, t("showPanel"), t("showPanelHint"), (on) => update({ panelVisible: on }));
  const collapseTile = toggleTile(ArrowRightToLine, t("tucksAway"), t("tucksAwayHint"), (on) => update({ tucksAway: on }));
  const cardTile = toggleTile(SquareMousePointer, t("showsCard"), t("showsCardHint"), (on) => update({ showsCard: on }));
  const lettersTile = toggleTile(Type, t("limitLetters"), t("limitLettersHint"), (on) => update({ limitLetters: on }));
  const remainingTile = toggleTile(Hourglass, t("showsRemaining"), t("showsRemainingHint"), (on) => update({ showsRemaining: on }));

  const steps = new Map<number, HTMLButtonElement>();
  const stepRow = el("div", "steps");
  stepRow.setAttribute("role", "radiogroup");
  stepRow.setAttribute("aria-label", t("warningAt"));
  for (const value of WARNING_CHOICES) {
    const step = button("step", el("span", "bar-slot", el("span", "bar")), el("span", "step-label", `${value}%`));
    step.setAttribute("role", "radio");
    step.addEventListener("click", () => {
      if (snapshot?.settings.warningAt !== value) update({ warningAt: value });
    });
    steps.set(value, step);
    stepRow.append(step);
  }
  arrowKeys(stepRow, () => [...steps.values()]);

  const panel = el(
    "section",
    "section",
    sectionHead(t("panel"), t("panelNote")),
    el("article", "card", heading(t("ringShows"), t("ringShowsHint")), ringRow),
    el("div", "tiles", panelTile.element, collapseTile.element, cardTile.element),
    el("div", "tiles two", lettersTile.element, remainingTile.element),
    el("article", "card threshold", heading(t("warningAt"), t("warningHint")), stepRow),
  );

  // Refresh and startup

  const interval = segmented<number>(
    t("interval"),
    INTERVALS.map((minutes) => [minutes, minutes === 0 ? t("intervalAuto") : t("intervalMinutes", String(minutes))]),
    (minutes) => update({ refreshMinutes: minutes }),
  );
  const refreshAll = button("button", icon(RotateCw, 15), el("span", undefined, t("refreshAll")));
  refreshAll.addEventListener("click", () => void invoke("refresh", { provider: null }));
  const login = toggle(t("launchAtLogin"), async (on) => {
    login.set(await invoke<boolean>("set_autostart", { enabled: on }));
  });
  void invoke<boolean>("get_autostart").then((on) => login.set(on));

  const intervalRow = el("div", "row stacked", el("div", "row-top", heading(t("interval"), t("intervalHint")), refreshAll), interval.element);
  const refresh = el(
    "section",
    "section",
    sectionHead(t("refreshGroup"), t("refreshNote")),
    el("article", "card rows", intervalRow, row(t("launchAtLogin"), t("launchAtLoginHint"), login.element)),
  );

  // Appearance

  const themeButtons = new Map<Theme, HTMLButtonElement>();
  const themeRow = el("div", "theme-options");
  themeRow.setAttribute("role", "radiogroup");
  themeRow.setAttribute("aria-label", t("theme"));
  for (const [theme, key, glyph] of THEME_CHOICES) {
    const option = button(
      "theme-option",
      themeThumb(theme),
      el("span", "theme-label", icon(glyph, 14), el("span", "theme-name", t(key)), el("span", "radio")),
    );
    option.setAttribute("role", "radio");
    option.addEventListener("click", () => {
      if (snapshot?.settings.theme !== theme) update({ theme });
    });
    themeButtons.set(theme, option);
    themeRow.append(option);
  }
  arrowKeys(themeRow, () => [...themeButtons.values()]);
  const railDark = toggle(t("railStaysDark"), (on) => update({ railStaysDark: on }));

  const appearance = el(
    "section",
    "section",
    sectionHead(t("appearance"), t("appearanceNote")),
    el(
      "article",
      "card appearance",
      heading(t("theme"), t("themeHint")),
      themeRow,
      row(t("railStaysDark"), t("railStaysDarkHint"), railDark.element),
    ),
  );

  const controls = el("div", "controls", services, panel, refresh, appearance);
  [services, panel, refresh, appearance].forEach((part, index) => reveal(part, index + 1));
  const element = el("section", "page", reveal(stage.element, 0), controls);

  function apply(snap: Snapshot) {
    snapshot = snap;
    const settings = snap.settings;

    chooser.hidden = settings.hasChosen;
    panel.hidden = refresh.hidden = appearance.hidden = !settings.hasChosen;
    for (const provider of PROVIDERS) {
      const account = snap.accounts.find((a) => a.provider === provider);
      let card = cards.get(provider);
      if (!card) {
        card = serviceCard(provider);
        cards.set(provider, card);
        servicesGrid.replaceChildren(...PROVIDERS.map((p) => cards.get(p)?.element).filter((e): e is HTMLElement => !!e));
      }
      if (account) applyService(card, account, snap);
    }

    check(ringButtons.values(), ringButtons.get(settings.ringShows));
    if (shownRing !== settings.ringShows) {
      shownRing = settings.ringShows;
      const glyph = ringButtons.get(settings.ringShows)?.querySelector("svg");
      glyph?.querySelectorAll<SVGCircleElement>("circle:not(.arc-track)").forEach((circle, index) => drawIn(circle, 0, index * 90));
    }

    panelTile.switch.set(settings.panelVisible);
    collapseTile.switch.set(settings.tucksAway);
    cardTile.switch.set(settings.showsCard);
    lettersTile.switch.set(settings.limitLetters);
    // Only a ring with both limits on it has two figures to tell apart.
    lettersTile.disable(!settings.ringShows.startsWith("both"), t("limitLettersOff"));
    remainingTile.switch.set(settings.showsRemaining);

    const at = WARNING_CHOICES.indexOf(settings.warningAt);
    check(steps.values(), steps.get(settings.warningAt));
    [...steps.values()].forEach((step, index) => {
      step.classList.toggle("at", index === at);
      step.classList.toggle("above", index > at);
    });

    interval.set(settings.refreshMinutes ?? 0);
    const busy = snap.accounts.some((account) => account.enabled && account.refreshing);
    refreshAll.disabled = busy || settings.enabled.length === 0;
    refreshAll.classList.toggle("spinning", busy);

    check(themeButtons.values(), themeButtons.get(settings.theme));
    railDark.set(settings.railStaysDark);

    stage.apply(snap);
  }

  return {
    element,
    apply,
    tick() {
      if (!snapshot) return;
      for (const account of snapshot.accounts) {
        const card = cards.get(account.provider);
        if (card) applyService(card, account, snapshot);
      }
      stage.tick();
    },
  };
}

// MARK: - Theme thumbnails

/** This window drawn small, in the colours a theme paints it. System is half of each. */
function themeThumb(theme: Theme): HTMLElement {
  const schemes: Scheme[] = theme === "system" ? ["light", "dark"] : [theme];
  const thumb = el("span", "theme-thumb", ...schemes.map(mini));
  thumb.setAttribute("aria-hidden", "true");
  return thumb;
}

/** The tabs, the stage with an arc on it, and three cards. */
function mini(scheme: Scheme): HTMLElement {
  const accent = svg("svg", { class: "mini-arc", width: 12, height: 12, viewBox: "0 0 12 12" });
  accent.append(arc(12, 4.8, -90, 200, { width: 2.4, round: false }));
  const cards = el("span", "mini-cards", el("span", "mini-card"), el("span", "mini-card"), el("span", "mini-card"));
  return el("span", `mini ${scheme}`, el("span", "mini-tabs"), el("span", "mini-row", el("span", "mini-stage"), cards, accent));
}

// MARK: - Ring glyphs

/**
 * What each choice looks like, small: the 5-hour limit in the accent, the
 * weekly one in lavender — the colours the rail draws them in, until a limit
 * passes the red line (`tint` in `format.ts`).
 */
function ringGlyph(shows: RingShows): SVGSVGElement {
  const size = 40;
  const glyph = svg("svg", { class: "ring-glyph", width: size, height: size, viewBox: `0 0 ${size} ${size}`, "aria-hidden": "true" });
  const five = { width: 4, className: "arc-five" };
  const week = { width: 4, className: "arc-week" };
  switch (shows) {
    case "fullest":
    case "fiveHour":
      glyph.append(track(size, 16, 4), arc(size, 16, -90, 150, five));
      break;
    case "weekly":
      glyph.append(track(size, 16, 4), arc(size, 16, -90, 65, week));
      break;
    case "bothSplit":
      glyph.append(arc(size, 16, 184, 172, five), arc(size, 16, 184, 172, { ...week, mirrored: true }));
      break;
    case "bothNested":
      glyph.append(
        track(size, 16.5, 3.5),
        arc(size, 16.5, -90, 150, { width: 3.5, className: "arc-five" }),
        track(size, 10, 3),
        arc(size, 10, -90, 65, { width: 3, className: "arc-week" }),
      );
      break;
    case "bothStacked": {
      // Two small rings side by side.
      const small = 14;
      const left = svg("g", { transform: `translate(4 ${(size - small) / 2})` });
      left.append(track(small, 5.5, 3), arc(small, 5.5, -90, 200, { width: 3, className: "arc-five" }));
      const right = svg("g", { transform: `translate(${size - small - 4} ${(size - small) / 2})` });
      right.append(track(small, 5.5, 3), arc(small, 5.5, -90, 110, { width: 3, className: "arc-week" }));
      glyph.append(left, right);
      break;
    }
  }
  return glyph;
}
