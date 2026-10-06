// Mirrors of what Rust sends (`src-tauri/src/model.rs`, `store.rs`,
// `panel.rs`). Rust is the source of truth; these only name the fields.

export type Provider = "claudeCode" | "codex";
/** One login: a provider's own is its id (`claudeCode`); an added Claude account is `claudeCode#<slot>`. */
export type AccountId = string;
export type WindowKind = "fiveHour" | "weekly" | "spend" | "other";
export type UsageState = "live" | "stale" | "unavailable";
export type Route = "endpoint" | "appServer" | "claudeCode";
export type CodexSource = "automatic" | "endpoint" | "tooling";
export type RingShows = "fullest" | "fiveHour" | "weekly" | "bothSplit" | "bothStacked" | "bothNested";
export type Theme = "system" | "light" | "dark";
/** With several Claude accounts, whose rings sit on the rail. */
export type RailFollows = "inUse" | "mostRoom" | "eachInTurn";
/** A theme settled: what System stands for at the moment. */
export type Scheme = Exclude<Theme, "system">;

export type Reason =
  | "notChecked"
  | "noLimitsReported"
  | "claudeSignInRequired"
  | "claudeLoginExpired"
  | "codexSignInRequired"
  | "codexServerFailed"
  | "unreachable"
  | "unreadableReply"
  | "rateLimited"
  | "serverError";

export interface UsageWindow {
  id: string;
  kind: WindowKind;
  scope: string | null;
  usedFraction: number;
  windowSeconds: number;
  /** Unix milliseconds. */
  resetsAt: number | null;
  isExhausted: boolean;
}

export interface ProviderUsage {
  provider: Provider;
  windows: UsageWindow[];
  observedAt: number | null;
  state: UsageState;
  reason: Reason | null;
  plan: string | null;
  creditBalance: string | null;
  origin: Route | null;
  isCached: boolean;
}

export interface Check {
  reason: Reason | null;
  origin: Route | null;
  checkedAt: number;
}

export interface AccountView {
  id: AccountId;
  provider: Provider;
  /** The product's name. Never translated. */
  name: string;
  /** What the reader named it, if they did. */
  label: string | null;
  /** Who it is signed in as. */
  email: string | null;
  /** What it is called where there is room for a word. */
  title: string;
  /** One Pulse added, rather than the product's own login. */
  added: boolean;
  /** The account Claude Code itself is signed in to. */
  inClaudeCode: boolean;
  /** Claude Code's own login, when it is an account added as well: shown once, as that one. */
  sameAs: AccountId | null;
  /** Its service is switched on, so it is read. */
  enabled: boolean;
  detected: boolean;
  /** The login it is read with, the home folder written `~`. */
  credentials: string;
  usage: ProviderUsage;
  refreshing: boolean;
  lastCheck: Check | null;
  /** Refused as too frequent: nothing is asked for it until then. Unix ms. */
  retryAt: number | null;
}

export interface Settings {
  /** Switched-on services, in rail order: `claudeCode` stands for every Claude account. */
  enabled: AccountId[];
  /** Slots of the Claude accounts added beyond Claude Code's own. */
  claudeAccounts: string[];
  accountLabels: Record<AccountId, string>;
  railFollows: RailFollows;
  /** The Claude account in use ran out: say which other one has room. */
  saysWhoIsFree: boolean;
  hasChosen: boolean;
  codexSource: CodexSource;
  refreshMinutes: number | null;
  showsRemaining: boolean;
  warningAt: number;
  ringShows: RingShows;
  limitLetters: boolean;
  panelVisible: boolean;
  tucksAway: boolean;
  showsCard: boolean;
  theme: Theme;
  railStaysDark: boolean;
  glass: boolean;
  /** Percent: 25, 50 or 75. */
  glassTransparency: number;
  checksForUpdates: boolean;
}

export interface Snapshot {
  accounts: AccountView[];
  settings: Settings;
  version: string;
  /** Light or dark as Windows has it, for a theme left to follow it. */
  systemTheme: Scheme;
  /** Whether there is a Claude Code to sign in to an account with. */
  claudeCodeFound: boolean;
  signIn: SignIn | null;
  switch: Switch | null;
}

export type SignInProblem = "noClaudeCode" | "timedOut" | "failed" | "alreadyAdded" | "tooMany";

/** A sign-in in the browser under way, or the last one, had it not worked. */
export interface SignIn {
  /** Signing in again to an account there is; null adding one. */
  account: AccountId | null;
  waiting: boolean;
  problem: SignInProblem | null;
  /** Claude Code's own words. Never translated. */
  detail: string | null;
}

export type SwitchProblem = "noLogin" | "unknown" | "failed";

/** Claude Code being handed an account's login, or the last time it was. */
export interface Switch {
  account: AccountId;
  waiting: boolean;
  problem: SwitchProblem | null;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Layout {
  /** Where the rail is in the window: down its left or right, or across its top. */
  side: "left" | "right" | "top";
  rail: Rect;
  /** Before the first item, from the rail's top down a side and its left end across the top. */
  padStart: number;
  /** One service's extent along the rail: its height down a side, its width across the top. */
  itemLength: number;
  itemSpacing: number;
  gap: number;
  cardWidth: number;
  pointerWidth: number;
  margin: number;
  visible: Rect;
  /** The screen edge the rail is fused to; null floating. Only a rail docked at the top lies across. */
  dock: "left" | "right" | "top" | null;
  /** Laid out while it is carried, on its way somewhere: animated to, and the drag goes on. */
  carried: boolean;
  /** How far a docked rail's ends reach beyond `rail`, along the edge. */
  flare: number;
  /** The window's size: the viewport, once it has caught up with a new one. */
  width: number;
  height: number;
  /** Which layout this is, for saying when it has been drawn. */
  generation: number;
}

export interface SettingsPatch {
  enabled?: AccountId[];
  railFollows?: RailFollows;
  saysWhoIsFree?: boolean;
  codexSource?: CodexSource;
  /** 0 is adaptive. */
  refreshMinutes?: number;
  showsRemaining?: boolean;
  warningAt?: number;
  ringShows?: RingShows;
  limitLetters?: boolean;
  panelVisible?: boolean;
  tucksAway?: boolean;
  showsCard?: boolean;
  theme?: Theme;
  railStaysDark?: boolean;
  glass?: boolean;
  glassTransparency?: number;
  checksForUpdates?: boolean;
}

/** A new version of Pulse itself (`src-tauri/src/updater.rs`). */
export type UpdateStatus =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "installing"
  | "failed"
  | "unsupported";

export interface UpdateInfo {
  status: UpdateStatus;
  current: string;
  version: string | null;
  notes: string | null;
  progress: number | null;
  checkedAt: number | null;
  error: string | null;
}
