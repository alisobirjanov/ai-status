// Several Claude accounts: which of them are one account, and which one the
// rail shows. The panel and the preview in Settings ask the same questions,
// so they always agree.

import type { AccountView, Snapshot } from "./types";

/** How long each account has the rail while they take turns. */
export const TURN_MS = 8_000;

/** Every Claude account once: Claude Code's own login is left out when it is one added too. */
export function claudeAccounts(snapshot: Snapshot): AccountView[] {
  return snapshot.accounts.filter((account) => account.provider === "claudeCode" && account.sameAs == null);
}

/** Whether there are figures to go by, banked or live. */
export function hasFigures(account: AccountView): boolean {
  return account.usage.state !== "unavailable" && account.usage.windows.length > 0;
}

/** How close an account is to its nearest limit, 0 to 1 and past it. */
export function fullness(account: AccountView): number {
  return Math.max(0, ...account.usage.windows.map((window) => window.usedFraction));
}

/** The account furthest from a limit, the one in use winning a tie. */
export function mostRoom(accounts: AccountView[]): AccountView | undefined {
  return accounts
    .filter(hasFigures)
    .reduce<AccountView | undefined>((best, account) => {
      if (!best) return account;
      const [a, b] = [fullness(account), fullness(best)];
      return a < b || (a === b && account.inClaudeCode) ? account : best;
    }, undefined);
}

/** The Claude account the rail follows at `now`. */
export function railClaude(snapshot: Snapshot, now: number): AccountView | undefined {
  const accounts = claudeAccounts(snapshot);
  const inUse = accounts.find((account) => account.inClaudeCode) ?? accounts.find(hasFigures) ?? accounts[0];
  switch (snapshot.settings.railFollows) {
    case "mostRoom":
      return mostRoom(accounts) ?? inUse;
    case "eachInTurn": {
      const read = accounts.filter(hasFigures);
      return read.length > 1 ? read[Math.floor(now / TURN_MS) % read.length] : inUse;
    }
    default:
      return inUse;
  }
}

/** What the rail shows, in its order: each service switched on, Claude as one account. */
export function railAccounts(snapshot: Snapshot, now = Date.now()): AccountView[] {
  const claude = railClaude(snapshot, now);
  const shown: AccountView[] = [];
  for (const account of snapshot.accounts) {
    if (!account.enabled) continue;
    if (account.provider !== "claudeCode") shown.push(account);
    else if (claude && !shown.includes(claude)) shown.push(claude);
  }
  return shown;
}

/** Whether the rail changes by itself, with nothing new read. */
export function takesTurns(snapshot: Snapshot): boolean {
  return snapshot.settings.railFollows === "eachInTurn" && claudeAccounts(snapshot).filter(hasFigures).length > 1;
}
