import type { IconName } from "../shell/icons";

// The panes of the Settings window (docs/design/components.md, "Settings"):
// the open project's first, then Ovra's own. Settings reopens on the pane it
// showed last; at first Agents, the one people need most (not the project's long Trust pane).

export type PaneId =
  | "trust"
  | "rules"
  | "network"
  | "keys"
  | "servers"
  | "schedules"
  | "editor"
  | "claude"
  | "setup"
  | "keyboard"
  | "notifications"
  | "phone"
  | "machines"
  | "accessibility"
  | "about";

export type Tint = "blue" | "purple" | "teal" | "amber" | "slate" | "grey" | "orange" | "green" | "red";

export type Pane = {
  id: PaneId;
  /** A noun: the pane's title and its name in the list. */
  title: string;
  /** One line on what the pane does. */
  line: string;
  group: "project" | "ovra";
  /** Its icon in the list, on a tile of `tint` (a `--tint-*` token in theme.css). */
  icon: IconName;
  tint: Tint;
  /** Other words the palette matches. */
  keywords: string[];
  /** Shown only while scheduled tasks are on (Settings > Agents). */
  optional?: boolean;
};

export const PANES: Pane[] = [
  {
    id: "trust",
    title: "Trust",
    icon: "shield",
    tint: "blue",
    line: "Whether Claude loads this project's own settings: its hooks, rules, commands and skills.",
    group: "project",
    keywords: ["hooks", "project settings"],
  },
  {
    id: "rules",
    title: "Rules",
    icon: "rules",
    tint: "purple",
    line: "What agents may do without you: Deny refuses, Ask asks you first, Auto allows. Anything else is asked.",
    group: "project",
    keywords: ["policies", "permissions"],
  },
  {
    id: "network",
    title: "Network",
    icon: "globe",
    tint: "teal",
    line: "Hosts sandboxed agents may reach; everything else is refused.",
    group: "project",
    keywords: ["proxy", "hosts", "allowlist"],
  },
  {
    id: "keys",
    title: "Keys",
    icon: "key",
    tint: "amber",
    line: "Tokens agents use without seeing them. Ovra asks you before an agent uses one.",
    group: "project",
    keywords: ["secrets", "token", "api key", "github", "credentials"],
  },
  {
    id: "servers",
    title: "Servers",
    icon: "server",
    tint: "slate",
    line: "Commands that start your app, so you and agents can open it.",
    group: "project",
    keywords: ["dev servers", "ports"],
  },
  {
    id: "schedules",
    title: "Schedules",
    icon: "clock",
    tint: "slate",
    line: "Agents that start by themselves at the days and times you set, while Ovra is open.",
    group: "project",
    keywords: ["scheduled tasks", "cron", "nightly", "timer", "recurring"],
    optional: true,
  },
  {
    id: "editor",
    title: "Editor",
    icon: "text",
    tint: "grey",
    line: "How text looks and when files save while you edit.",
    group: "ovra",
    keywords: ["font", "text size", "indent", "tab size", "wrap", "autosave", "auto save"],
  },
  {
    id: "claude",
    title: "Agents",
    icon: "sparkles",
    tint: "orange",
    line: "The agents on this computer that run your tasks, and who pays for them.",
    group: "ovra",
    keywords: ["account", "billing", "path", "sign in", "agents at once", "parallel", "limit", "templates", "model"],
  },
  {
    id: "setup",
    title: "Setup check",
    icon: "checked",
    tint: "green",
    line: "Whether this computer has what Ovra needs, with one way to fix each problem.",
    group: "ovra",
    keywords: ["check setup", "doctor", "troubleshoot", "install", "git", "gh", "github", "sandbox", "bubblewrap"],
  },
  {
    id: "notifications",
    title: "Notifications",
    icon: "bell",
    tint: "red",
    line: "How Ovra tells you that an agent needs you.",
    group: "ovra",
    keywords: ["alerts", "toasts"],
  },
  {
    id: "phone",
    title: "Phone",
    icon: "phone",
    tint: "blue",
    line: "See and answer your agents from your phone, on the same Wi-Fi.",
    group: "ovra",
    keywords: ["mobile", "remote", "pair", "qr", "companion"],
  },
  {
    id: "machines",
    title: "Remote machines",
    icon: "monitor",
    tint: "slate",
    line: "Run agents on another Linux computer you reach over SSH, each in a window of its own.",
    group: "ovra",
    keywords: ["ssh", "server", "remote", "connect to remote machine", "host key", "fingerprint"],
  },
  {
    id: "keyboard",
    title: "Keyboard",
    icon: "keyboard",
    tint: "grey",
    line: "Every shortcut, by where it works.",
    group: "ovra",
    keywords: ["shortcuts", "keybindings", "hotkeys"],
  },
  {
    id: "accessibility",
    title: "Accessibility",
    icon: "person",
    tint: "blue",
    line: "Motion, contrast and colours follow your system's settings.",
    group: "ovra",
    keywords: ["screen reader", "a11y"],
  },
  {
    id: "about",
    title: "About",
    icon: "info",
    tint: "grey",
    line: "Which Ovra this is.",
    group: "ovra",
    keywords: ["version", "licenses"],
  },
];

/** The pane Settings shows, and whether Agents' extra features are unfolded; kept while the window is closed. */
export const settings = $state<{ pane: PaneId; extra: boolean }>({ pane: "claude", extra: false });

/** The panes there are: the project's only with a project open, Schedules only while they are on. */
export function panesFor(project: boolean, schedules = false): Pane[] {
  return PANES.filter((pane) => (project || pane.group === "ovra") && (!pane.optional || schedules));
}

/** The panes whose title, line or keywords hold every word of `query`, titles that start with it first. */
export function findPanes(panes: Pane[], query: string): Pane[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return panes;
  const text = (pane: Pane) => [pane.title, pane.line, ...pane.keywords].join(" ").toLowerCase();
  const found = panes.filter((pane) => words.every((word) => text(pane).includes(word)));
  const first = words.join(" ");
  return [
    ...found.filter((pane) => pane.title.toLowerCase().startsWith(first)),
    ...found.filter((pane) => !pane.title.toLowerCase().startsWith(first)),
  ];
}
