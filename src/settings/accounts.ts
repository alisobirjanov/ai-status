// Accounts: every Claude account Pulse reads, side by side — how much of
// each limit is gone, when it comes back, and which one Claude Code is in.
// One is added by signing in in the browser, and handed to Claude Code with
// a click; nobody types a command.

import { invoke } from "@tauri-apps/api/core";
import { ArrowRightLeft, CircleAlert, CircleCheck, Ellipsis, KeyRound, LoaderCircle, Pencil, Plus, Power, RotateCw, Trash2, type IconNode } from "lucide";

import { claudeAccounts, hasFigures, mostRoom } from "../shared/accounts";
import { duration, failedText, hiddenEmail, isSpent, percentValue, refreshHint, relative, resetLine, shortWindowName, shownFraction, tint, waitLeft } from "../shared/format";
import { icon as productMark } from "../shared/icons";
import { t, type StringKey } from "../shared/i18n";
import { svg } from "../shared/rail";
import type { AccountView, RailFollows, Reason, SettingsPatch, SignIn, SignInProblem, Snapshot, SwitchProblem, UsageWindow } from "../shared/types";
import { arc, button, countTo, dashLength, drawIn, el, heading, icon, reveal, row, sectionHead, segmented, toggle } from "./dom";
import type { Page } from "./general";

/** `MAX_ADDED` in `accounts.rs`: Claude accounts beyond Claude Code's own. */
const MAX_ADDED = 4;
const RING = 52;
const RING_WIDTH = 5.5;
const RING_RADIUS = (RING - RING_WIDTH) / 2;

const SIGN_IN_PROBLEMS: Record<SignInProblem, StringKey> = {
  noClaudeCode: "signInNoClaudeCode",
  timedOut: "signInTimedOut",
  failed: "signInFailed",
  alreadyAdded: "signInAlreadyAdded",
  tooMany: "signInTooMany",
};

const SWITCH_PROBLEMS: Record<SwitchProblem, StringKey> = {
  noLogin: "switchNoLogin",
  unknown: "switchUnknown",
  failed: "switchFailed",
};

function signInProblem(signIn: SignIn): string {
  const text = signIn.problem ? t(SIGN_IN_PROBLEMS[signIn.problem]) : "";
  return signIn.detail ? `${text} ${signIn.detail}` : text;
}

/** A login that has gone, which signing in again would bring back. */
function loginGone(account: AccountView): Extract<Reason, "claudeLoginExpired" | "claudeSignInRequired"> | null {
  if (account.usage.state === "live") return null;
  const reason = account.usage.state === "unavailable" ? account.usage.reason : account.lastCheck?.reason;
  return reason === "claudeLoginExpired" || reason === "claudeSignInRequired" ? reason : null;
}

/** The 5-hour and weekly limits, as the rail has them; whatever there is otherwise. */
function limits(account: AccountView): UsageWindow[] {
  const windows = account.usage.windows;
  const pair = (["fiveHour", "weekly"] as const)
    .map((kind) => windows.find((window) => window.kind === kind && !window.scope))
    .filter((window): window is UsageWindow => !!window);
  return pair.length ? pair : windows.slice(0, 2);
}

/** The figure a limit shows, counted the way the reader chose. */
function shownPercent(window: UsageWindow, remaining: boolean): number {
  if (isSpent(window)) return remaining ? 0 : 100;
  return percentValue(shownFraction(window, remaining));
}

/** "5-hour 30% · weekly 28%". */
function figuresLine(account: AccountView, remaining: boolean): string {
  const shown = limits(account);
  const [five, week] = shown;
  if (five?.kind === "fiveHour" && week?.kind === "weekly") {
    return t("bothFigures", `${shownPercent(five, remaining)}%`, `${shownPercent(week, remaining)}%`);
  }
  return shown.map((window) => `${shortWindowName(window).toLowerCase()} ${shownPercent(window, remaining)}%`).join(" · ");
}

/** When an account with a spent limit has room again: once every spent limit is back. */
function backAt(account: AccountView): { at: number; window: UsageWindow } | null {
  const spent = account.usage.windows.filter((window) => isSpent(window) && window.resetsAt != null && window.resetsAt > Date.now());
  if (!spent.length) return null;
  const last = spent.reduce((a, b) => (b.resetsAt! > a.resetsAt! ? b : a));
  return { at: last.resetsAt!, window: last };
}

function checkedText(account: AccountView): string {
  if (account.refreshing) return t("checking");
  const at = account.lastCheck?.checkedAt ?? account.usage.observedAt;
  return at != null ? relative(at, "short") : t("checkedNever");
}

function meta(label: string, value: HTMLElement): HTMLElement {
  return el("div", "meta", el("span", "meta-label", label), value);
}

// MARK: - A limit as a ring

interface Meter {
  element: HTMLElement;
  arc: SVGCircleElement;
  label: HTMLElement;
  note: HTMLElement;
  figure: HTMLElement;
  reset: HTMLElement;
}

function meter(): Meter {
  const ring = svg("svg", { class: "meter-ring", width: RING, height: RING, viewBox: `0 0 ${RING} ${RING}`, "aria-hidden": "true" });
  const used = arc(RING, RING_RADIUS, -90, 0, { width: RING_WIDTH, className: "meter-arc" });
  ring.append(svg("circle", { cx: RING / 2, cy: RING / 2, r: RING_RADIUS, fill: "none", "stroke-width": RING_WIDTH, class: "meter-track" }), used);
  const label = el("span", "meter-label");
  const note = el("span", "meter-note", t("limitReached"));
  const figure = el("div", "meter-figure");
  const reset = el("div", "meter-reset");
  const element = el("div", "meter", ring, el("div", "meter-text", el("div", "meter-top", label, note), figure, reset));
  return { element, arc: used, label, note, figure, reset };
}

function fillMeter(meter: Meter, window: UsageWindow, snapshot: Snapshot) {
  const settings = snapshot.settings;
  const percent = shownPercent(window, settings.showsRemaining);
  const spent = isSpent(window);
  meter.label.textContent = shortWindowName(window);
  meter.note.hidden = !spent;

  const circumference = 2 * Math.PI * RING_RADIUS;
  const from = dashLength(meter.arc);
  meter.arc.setAttribute("stroke-dasharray", `${(circumference * percent) / 100} ${circumference}`);
  meter.arc.style.stroke = tint(window, settings.warningAt);
  meter.arc.style.visibility = percent > 0 ? "" : "hidden";
  if (Math.abs(from - dashLength(meter.arc)) > 0.5) drawIn(meter.arc, from, 0, 800);
  countTo(meter.figure, percent, (value) => `${value}%`, meter.figure.dataset.value ? undefined : 0);

  // A spent limit says when it is back; the rest, when they reset.
  const ms = window.resetsAt != null ? window.resetsAt - Date.now() : 0;
  meter.reset.textContent = spent && ms > 0 ? t("backIn", duration(ms)) : resetLine(window);
}

// MARK: - An account

interface Row {
  element: HTMLElement;
  glyph: HTMLElement;
  title: HTMLElement;
  badge: HTMLElement;
  found: HTMLElement;
  plan: HTMLElement;
  checked: HTMLElement;
  meters: HTMLElement;
  five: Meter;
  week: Meter;
  divider: HTMLElement;
  box: HTMLElement;
  boxIcon: HTMLElement;
  boxTitle: HTMLElement;
  boxText: HTMLElement;
  boxAside: HTMLElement;
  boxButton: HTMLButtonElement;
  boxCancel: HTMLButtonElement;
  use: HTMLButtonElement;
  refresh: HTMLButtonElement;
  remove?: HTMLButtonElement;
  closeMenu(): void;
  /** What the box shows now, so its buttons do what it asks. */
  setBox(state: BoxState | null): void;
  /** Whose it is, or `null` while nobody is signed in to it. */
  setEmail(address: string | null): void;
}

/** What the box in place of the rings says, when it is there. */
interface BoxState {
  glyph: IconNode;
  spin?: boolean;
  tone: "quiet" | "problem";
  title: string;
  text?: string;
  aside?: string;
  button?: StringKey;
  cancel?: boolean;
  /** What the button and Cancel do, when not signing in. */
  onButton?: () => void;
  onCancel?: () => void;
}

export interface AccountsPage extends Page {
  shown(): void;
}

export function accounts(update: (patch: SettingsPatch) => void): AccountsPage {
  let snapshot: Snapshot | null = null;
  /** The open "…" menu's way of closing, so a click anywhere else closes it. */
  let openMenu: (() => void) | null = null;
  document.addEventListener("pointerdown", (event) => {
    if (openMenu && !(event.target as Element).closest?.(".account-actions")) openMenu();
  });

  // Head

  const refreshAll = button("button", icon(RotateCw, 15), el("span", undefined, t("refreshAccounts")));
  refreshAll.addEventListener("click", () => {
    if (snapshot) for (const account of claudeAccounts(snapshot)) void invoke("refresh", { account: account.id });
  });
  const add = button("button accent", icon(Plus, 15), el("span", undefined, t("addAccount")));
  add.addEventListener("click", () => void invoke("sign_in_claude", { account: null }));
  const head = el(
    "div",
    "accounts-head",
    el("div", "accounts-title", el("h2", undefined, t("claudeAccounts")), el("p", "section-note", t("claudeAccountsNote"))),
    el("div", "head-actions", refreshAll, add),
  );

  const switchOn = button("button", el("span", undefined, t("switchOn")));
  switchOn.addEventListener("click", () => update({ enabled: [...(snapshot?.settings.enabled ?? []), "claudeCode"] }));
  const off = el("div", "callout claude-off", el("span", "callout-icon", icon(Power, 18)), heading(t("claudeOff")), switchOn);

  const switchedTitle = el("span");
  const gotIt = button("button", el("span", undefined, t("gotIt")));
  gotIt.addEventListener("click", () => void invoke("dismiss_switch"));
  const switched = el("div", "callout switched", el("span", "callout-icon", icon(CircleCheck, 18)), heading(switchedTitle, t("switchedNote")), gotIt);

  // Summary

  function cell(label: StringKey) {
    const value = el("div", "summary-value");
    const sub = el("div", "summary-sub");
    return { element: el("div", "summary-cell", el("div", "summary-label", t(label)), value, sub), value, sub };
  }
  const inUseCell = cell("summaryInUse");
  const roomCell = cell("summaryMostRoom");
  const soonestCell = cell("summaryBackSoonest");
  const signInCell = cell("summaryNeedsSignIn");
  const summary = el("div", "card summary", inUseCell.element, roomCell.element, soonestCell.element, signInCell.element);

  function applySummary(list: AccountView[], snap: Snapshot) {
    const remaining = snap.settings.showsRemaining;
    const set = (target: ReturnType<typeof cell>, value: string, sub: string) => {
      target.value.textContent = value;
      target.value.title = value;
      target.sub.textContent = sub;
    };

    const inUse = list.find((account) => account.inClaudeCode);
    if (inUse) set(inUseCell, inUse.title, hasFigures(inUse) ? figuresLine(inUse, remaining) : "");
    else set(inUseCell, t("nobody"), t("claudeCodeSignedOut"));

    const room = mostRoom(list);
    set(roomCell, room?.title ?? t("nobody"), room ? figuresLine(room, remaining) : "");

    const soonest = list
      .map((account) => ({ account, back: backAt(account) }))
      .filter((each) => each.back)
      .sort((a, b) => a.back!.at - b.back!.at)[0];
    if (soonest) {
      const { window, at } = soonest.back!;
      set(soonestCell, soonest.account.title, t("freeIn", shortWindowName(window), duration(at - Date.now())));
    } else set(soonestCell, t("nobody"), t("noLimitReached"));

    const gone = list.filter(loginGone);
    if (gone.length) {
      const seen = gone[0].usage.observedAt;
      set(signInCell, gone.map((account) => account.title).join(", "), seen != null ? t("lastRead", relative(seen)) : "");
    } else set(signInCell, t("nobody"), t("everyLoginWorks"));
  }

  // The accounts

  const rows = new Map<string, Row>();
  const list = el("div", "account-list");

  function accountRow(account: AccountView): Row {
    const id = account.id;
    const glyph = el("span", "glyph", productMark("claudeCode", 18));
    const title = el("h3", "account-title");
    const badge = el("span", "account-badge", t("inClaudeCode"));
    // Where the badge would be, the way to it: hand this account to Claude Code itself.
    const use = button("use-pill", el("span", "use-icon"), el("span", undefined, t("useInClaudeCode")));
    use.title = t("useInClaudeCodeHint");
    use.addEventListener("click", () => void invoke("use_account", { account: id }));
    // Whose it is, kept to its first letters until asked for: a settings
    // window is often on a screen somebody else can see.
    const email = button("email");
    let address: string | null = null;
    let shown = false;
    const drawEmail = () => {
      email.disabled = address == null;
      email.textContent = address == null ? t("notSignedIn") : shown ? address : hiddenEmail(address);
      email.title = address == null ? "" : t(shown ? "hideEmail" : "showEmail");
    };
    email.addEventListener("click", () => {
      shown = !shown;
      drawEmail();
    });
    const found = el("p", "found", el("span", "dot"), email);
    const plan = el("span", "meta-value");
    const checked = el("span", "meta-value");
    const identity = el(
      "div",
      "account-id",
      el("div", "account-top", glyph, el("div", "account-name", el("div", "account-name-line", title, badge, use), found)),
      el("div", "account-metas", meta(t("plan"), plan), meta(t("checked"), checked)),
    );

    const five = meter();
    const week = meter();
    const divider = el("span", "account-divider");
    const meters = el("div", "account-meters", five.element, divider, week.element);

    // In place of the rings: a login gone, a sign-in under way, or why there are no figures.
    const boxIcon = el("span", "box-icon");
    const boxTitle = el("p", "box-title");
    const boxText = el("p", "box-text");
    const boxAside = el("span", "box-aside");
    const boxButton = button("button small");
    boxButton.addEventListener("click", () => (shownBox?.onButton ?? (() => void invoke("sign_in_claude", { account: id })))());
    const boxCancel = button("link-button", t("cancel"));
    boxCancel.addEventListener("click", () => (shownBox?.onCancel ?? (() => void invoke("cancel_sign_in")))());
    const box = el(
      "div",
      "account-box",
      boxIcon,
      el("div", "box-words", boxTitle, boxText),
      el("div", "box-end", boxAside, el("div", "box-buttons", boxCancel, boxButton)),
    );

    const refresh = button("icon-button", icon(RotateCw, 15));
    refresh.setAttribute("aria-label", `${t("refresh")} ${account.title}`);
    refresh.title = t("refresh");
    refresh.addEventListener("click", () => void invoke("refresh", { account: id }));

    // "…": rename, and for one Pulse added, remove.
    const more = button("icon-button", icon(Ellipsis, 15));
    more.setAttribute("aria-label", t("moreActions"));
    more.setAttribute("aria-haspopup", "menu");
    more.setAttribute("aria-expanded", "false");
    more.title = t("moreActions");
    const menu = el("div", "menu");
    menu.setAttribute("role", "menu");
    menu.hidden = true;
    const renameItem = button("menu-item", icon(Pencil, 14), el("span", undefined, t("rename")));
    renameItem.setAttribute("role", "menuitem");
    menu.append(renameItem);

    let armTimer: number | undefined;
    let remove: HTMLButtonElement | undefined;
    const disarm = () => {
      window.clearTimeout(armTimer);
      armTimer = undefined;
      if (remove) {
        remove.classList.remove("armed");
        remove.lastElementChild!.textContent = t("removeAccount");
      }
    };
    const closeMenu = () => {
      menu.hidden = true;
      more.setAttribute("aria-expanded", "false");
      disarm();
      if (openMenu === closeMenu) openMenu = null;
    };
    more.addEventListener("click", () => {
      if (!menu.hidden) return closeMenu();
      openMenu?.();
      menu.hidden = false;
      more.setAttribute("aria-expanded", "true");
      openMenu = closeMenu;
      renameItem.focus();
    });
    menu.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      closeMenu();
      more.focus();
    });

    // Named in place: the name becomes a field until Enter or a click away.
    renameItem.addEventListener("click", () => {
      closeMenu();
      const field = el("input", "name-field");
      field.value = snapshot?.settings.accountLabels[id] ?? title.textContent ?? "";
      field.maxLength = 32;
      field.setAttribute("aria-label", t("accountName"));
      let done = false;
      const finish = (keep: boolean) => {
        if (done) return;
        done = true;
        field.replaceWith(title);
        if (keep) void invoke("rename_account", { account: id, label: field.value });
      };
      field.addEventListener("keydown", (event) => {
        if (event.key === "Enter") finish(true);
        if (event.key === "Escape") finish(false);
      });
      field.addEventListener("blur", () => finish(true));
      title.replaceWith(field);
      field.focus();
      field.select();
    });

    // Claude Code's own login is not Pulse's to remove. Two clicks: one to ask, one to mean it.
    if (account.added) {
      const item = button("menu-item danger", icon(Trash2, 14), el("span", undefined, t("removeAccount")));
      item.setAttribute("role", "menuitem");
      item.addEventListener("click", () => {
        if (armTimer === undefined) {
          item.classList.add("armed");
          item.lastElementChild!.textContent = t("removeConfirm");
          armTimer = window.setTimeout(disarm, 4000);
          return;
        }
        closeMenu();
        void invoke("remove_account", { account: id });
      });
      menu.append(item);
      remove = item;
    }

    const element = el(
      "article",
      "card account-row",
      identity,
      el("span", "account-divider"),
      el("div", "account-body", meters, box),
      el("div", "account-actions", refresh, more, menu),
    );
    let shownBox: BoxState | null = null;
    return {
      element,
      glyph,
      title,
      badge,
      found,
      plan,
      checked,
      meters,
      five,
      week,
      divider,
      box,
      boxIcon,
      boxTitle,
      boxText,
      boxAside,
      boxButton,
      boxCancel,
      use,
      refresh,
      remove,
      closeMenu,
      setBox: (state: BoxState | null) => (shownBox = state),
      setEmail: (next: string | null) => {
        // Another login in its place is hidden again.
        if (next !== address) shown = false;
        address = next;
        drawEmail();
      },
    };
  }

  /** What stands in for the rings, if anything does. */
  function boxState(account: AccountView, snap: Snapshot): BoxState | null {
    const switched = snap.switch?.account === account.id ? snap.switch : null;
    if (switched?.problem) {
      return {
        glyph: CircleAlert,
        tone: "problem",
        title: t(SWITCH_PROBLEMS[switched.problem]),
        button: "tryAgain",
        cancel: true,
        onButton: () => void invoke("use_account", { account: account.id }),
        onCancel: () => void invoke("dismiss_switch"),
      };
    }
    const mine = snap.signIn?.account === account.id ? snap.signIn : null;
    if (mine?.waiting) return { glyph: LoaderCircle, spin: true, tone: "quiet", title: t("signInWaiting"), cancel: true };
    if (mine?.problem) return { glyph: CircleAlert, tone: "problem", title: signInProblem(mine), button: "tryAgain", cancel: true };

    const gone = loginGone(account);
    if (gone) {
      const seen = account.usage.observedAt;
      const fullest = limits(account).sort((a, b) => b.usedFraction - a.usedFraction)[0];
      const figure = fullest ? ` · ${shortWindowName(fullest).toLowerCase()} ${shownPercent(fullest, snap.settings.showsRemaining)}%` : "";
      return {
        glyph: KeyRound,
        tone: "quiet",
        title: t(gone === "claudeLoginExpired" ? "signInExpired" : "notSignedIn"),
        text: t(account.added ? "expiredBodyAdded" : "expiredBodyMain"),
        aside: seen != null ? `${t("lastSeen", relative(seen))}${figure}` : undefined,
        button: snap.claudeCodeFound ? "signInAgain" : undefined,
      };
    }
    if (hasFigures(account) && limits(account).length) return null;
    if (account.refreshing || account.usage.reason === "notChecked" || account.usage.reason == null) {
      return { glyph: LoaderCircle, spin: account.refreshing, tone: "quiet", title: account.refreshing ? t("checking") : t("notChecked") };
    }
    return { glyph: CircleAlert, tone: "problem", title: failedText(account.usage.reason, account) };
  }

  function applyRow(card: Row, account: AccountView, snap: Snapshot) {
    const on = account.enabled;
    const gone = loginGone(account) != null;
    card.element.classList.toggle("in-use", account.inClaudeCode && on);
    card.element.classList.toggle("dim", gone || !on);
    // Not while it is being renamed: the field stands in its place.
    if (card.title.parentElement) card.title.textContent = account.title;
    card.title.title = account.title;
    card.badge.hidden = !account.inClaudeCode;
    card.setEmail(account.email);
    card.found.classList.toggle("missing", gone || account.usage.state === "unavailable");
    card.plan.textContent = account.usage.plan ?? "–";
    card.checked.textContent = checkedText(account);
    card.refresh.disabled = !on || account.refreshing || waitLeft(account) > 0;
    card.refresh.classList.toggle("spinning", account.refreshing);
    card.refresh.title = refreshHint(account);
    card.refresh.setAttribute("aria-label", `${t("refresh")} ${account.title}`);

    // Any account Pulse added can be the one Claude Code is in, so long as its login works.
    const switching = snap.switch?.waiting === true;
    const mine = switching && snap.switch?.account === account.id;
    card.use.hidden = !account.added || account.inClaudeCode || gone;
    card.use.disabled = switching || !!snap.signIn?.waiting;
    card.use.classList.toggle("spin", mine);
    card.use.firstElementChild!.replaceChildren(icon(mine ? LoaderCircle : ArrowRightLeft, 14));
    card.use.lastElementChild!.textContent = t(mine ? "switching" : "useInClaudeCode");

    const state = boxState(account, snap);
    card.setBox(state);
    card.meters.hidden = !!state;
    card.box.hidden = !state;
    if (state) {
      card.box.dataset.tone = state.tone;
      card.boxIcon.replaceChildren(icon(state.glyph, 16));
      card.boxIcon.classList.toggle("spin", !!state.spin);
      card.boxTitle.textContent = state.title;
      card.boxText.textContent = state.text ?? "";
      card.boxText.hidden = !state.text;
      card.boxAside.textContent = state.aside ?? "";
      card.boxAside.hidden = !state.aside;
      card.boxButton.hidden = !state.button;
      card.boxButton.textContent = state.button ? t(state.button) : "";
      card.boxButton.disabled = !!snap.signIn?.waiting || switching;
      card.boxCancel.hidden = !state.cancel;
    } else {
      const [five, week] = limits(account);
      fillMeter(card.five, five, snap);
      card.week.element.hidden = card.divider.hidden = !week;
      if (week) fillMeter(card.week, week, snap);
    }
  }

  // Adding one: under way, or why it didn't work. Otherwise, a way to add one.
  const pendingText = el("p", "found");
  const pendingCancel = button("button", el("span", undefined, t("cancel")));
  pendingCancel.addEventListener("click", () => void invoke("cancel_sign_in"));
  const pendingRetry = button("button accent", el("span", undefined, t("tryAgain")));
  pendingRetry.addEventListener("click", () => void invoke("sign_in_claude", { account: null }));
  const pendingGlyph = el("span", "glyph");
  const pending = el(
    "article",
    "card account-row pending",
    el("div", "account-id", el("div", "account-top", pendingGlyph, el("div", "account-name", el("h3", "account-title", t("newAccount")), pendingText))),
    el("span", "account-divider"),
    el("p", "account-body pending-hint", t("addAccountHint")),
    el("div", "account-actions", pendingCancel, pendingRetry),
  );

  const addText = el("span");
  const addRow = button("add-row", icon(Plus, 16), addText);
  addRow.addEventListener("click", () => void invoke("sign_in_claude", { account: null }));

  function applyAdding(snap: Snapshot) {
    const signIn = snap.signIn?.account == null ? snap.signIn : null;
    const waiting = !!signIn?.waiting;
    const full = snap.settings.claudeAccounts.length >= MAX_ADDED;
    const why = !snap.claudeCodeFound ? t("signInNoClaudeCode") : full ? t("signInTooMany") : "";
    add.disabled = !!why || !!snap.signIn?.waiting;
    add.title = why;

    pending.hidden = !waiting && !signIn?.problem;
    pending.classList.toggle("problem", !waiting && !!signIn?.problem);
    pendingGlyph.replaceChildren(icon(waiting ? LoaderCircle : CircleAlert, 18));
    pendingGlyph.classList.toggle("spin", waiting);
    pendingText.textContent = waiting ? t("signInWaiting") : signIn ? signInProblem(signIn) : "";
    pendingRetry.hidden = waiting || !!why;

    // Only one account so far: say what adding the others is for.
    addRow.hidden = !pending.hidden || snap.settings.claudeAccounts.length > 0;
    addRow.disabled = add.disabled;
    addText.textContent = why || t("addOtherAccounts");
  }

  function applyList(snap: Snapshot): AccountView[] {
    const shown = claudeAccounts(snap);
    for (const id of [...rows.keys()]) {
      if (!shown.some((account) => account.id === id)) {
        rows.get(id)!.closeMenu();
        rows.delete(id);
      }
    }
    const children: HTMLElement[] = [];
    for (const account of shown) {
      let card = rows.get(account.id);
      if (!card) {
        card = accountRow(account);
        rows.set(account.id, card);
      }
      applyRow(card, account, snap);
      children.push(card.element);
    }
    applyAdding(snap);
    children.push(pending, addRow);
    const same = children.length === list.children.length && children.every((child, index) => list.children[index] === child);
    if (!same) list.replaceChildren(...children);
    return shown;
  }

  // With several accounts

  const follows = segmented<RailFollows>(
    t("railFollows"),
    [
      ["inUse", t("followsInUse")],
      ["mostRoom", t("followsMostRoom")],
      ["eachInTurn", t("followsEachInTurn")],
    ],
    (value) => update({ railFollows: value }),
  );
  const saysFree = toggle(t("saysWhoIsFree"), (on) => update({ saysWhoIsFree: on }));
  const several = el(
    "section",
    "section",
    sectionHead(t("severalAccounts"), t("severalAccountsNote")),
    el(
      "article",
      "card rows",
      row(t("railFollows"), t("railFollowsHint"), follows.element),
      row(t("saysWhoIsFree"), t("saysWhoIsFreeHint"), saysFree.element),
    ),
  );

  const top = el("section", "section accounts", head, off, switched, summary, list);
  const element = el("section", "page wide", reveal(top, 0), reveal(several, 1));

  function apply(snap: Snapshot) {
    snapshot = snap;
    const on = snap.settings.enabled.includes("claudeCode");
    off.hidden = on;
    const shown = applyList(snap);
    const done = snap.switch && !snap.switch.waiting && !snap.switch.problem ? shown.find((a) => a.id === snap.switch!.account && a.inClaudeCode) : undefined;
    switched.hidden = !done;
    if (done) switchedTitle.textContent = t("switchedTitle", done.title);
    summary.hidden = shown.length < 2;
    if (shown.length >= 2) applySummary(shown, snap);
    const busy = shown.some((account) => account.refreshing);
    refreshAll.disabled = !on || busy || shown.length === 0 || shown.every((account) => waitLeft(account) > 0);
    refreshAll.classList.toggle("spinning", busy);
    follows.set(snap.settings.railFollows);
    saysFree.set(snap.settings.saysWhoIsFree);
  }

  return {
    element,
    apply,
    tick() {
      if (snapshot) apply(snapshot);
    },
    /** The rings fill and the figures count up as the page comes in. */
    shown() {
      for (const card of rows.values()) {
        [card.five, card.week].forEach((each, index) => {
          if (each.element.offsetParent === null) return;
          drawIn(each.arc, 0, 120 + index * 90, 800);
          countTo(each.figure, Number(each.figure.dataset.value ?? 0), (value) => `${value}%`, 0);
        });
      }
    },
  };
}
