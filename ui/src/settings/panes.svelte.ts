// The panes of the Settings view (docs/design/components.md, "Settings"):
// the open project's first, then Ovra's own. Settings reopens on the pane it
// showed last.

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

export type Pane = {
  id: PaneId;
  /** A noun: the pane's title and its name in the list. */
  title: string;
  /** One line on what the pane does. */
  line: string;
  group: "project" | "ovra";
  /** Other words the palette matches. */
  keywords: string[];
  /** Shown only while scheduled tasks are on (Settings > Agents). */
  optional?: boolean;
};

export const PANES: Pane[] = [
  {
    id: "trust",
    title: "Trust",
    line: "Whether Claude loads this project's own settings: its hooks, rules, commands and skills.",
    group: "project",
    keywords: ["hooks", "project settings"],
  },
  {
    id: "rules",
    title: "Rules",
    line: "What agents may do without you: Deny refuses, Ask asks you first, Auto allows. Anything else is asked.",
    group: "project",
    keywords: ["policies", "permissions"],
  },
  {
    id: "network",
    title: "Network",
    line: "Hosts sandboxed agents may reach; everything else is refused.",
    group: "project",
    keywords: ["proxy", "hosts", "allowlist"],
  },
  {
    id: "keys",
    title: "Keys",
    line: "Tokens agents use without seeing them. Ovra asks you before an agent uses one.",
    group: "project",
    keywords: ["secrets", "token", "api key", "github", "credentials"],
  },
  {
    id: "servers",
    title: "Servers",
    line: "Commands that start your app, so you and agents can open it.",
    group: "project",
    keywords: ["dev servers", "ports"],
  },
  {
    id: "schedules",
    title: "Schedules",
    line: "Agents that start by themselves at the days and times you set, while Ovra is open.",
    group: "project",
    keywords: ["scheduled tasks", "cron", "nightly", "timer", "recurring"],
    optional: true,
  },
  {
    id: "editor",
    title: "Editor",
    line: "How text looks and when files save while you edit.",
    group: "ovra",
    keywords: ["font", "text size", "indent", "tab size", "wrap", "autosave", "auto save"],
  },
  {
    id: "claude",
    title: "Agents",
    line: "The agents on this computer that run your tasks, and who pays for them.",
    group: "ovra",
    keywords: ["account", "billing", "path", "sign in", "agents at once", "parallel", "limit", "templates", "model"],
  },
  {
    id: "setup",
    title: "Setup check",
    line: "Whether this computer has what Ovra needs, with one way to fix each problem.",
    group: "ovra",
    keywords: ["check setup", "doctor", "troubleshoot", "install", "git", "gh", "github", "sandbox", "bubblewrap"],
  },
  {
    id: "notifications",
    title: "Notifications",
    line: "How Ovra tells you that an agent needs you.",
    group: "ovra",
    keywords: ["alerts", "toasts"],
  },
  {
    id: "phone",
    title: "Phone",
    line: "See and answer your agents from your phone, on the same Wi-Fi.",
    group: "ovra",
    keywords: ["mobile", "remote", "pair", "qr", "companion"],
  },
  {
    id: "machines",
    title: "Remote machines",
    line: "Run agents on another Linux computer you reach over SSH, each in a window of its own.",
    group: "ovra",
    keywords: ["ssh", "server", "remote", "connect to remote machine", "host key", "fingerprint"],
  },
  {
    id: "keyboard",
    title: "Keyboard",
    line: "Every shortcut, by where it works.",
    group: "ovra",
    keywords: ["shortcuts", "keybindings", "hotkeys"],
  },
  {
    id: "accessibility",
    title: "Accessibility",
    line: "Motion, contrast and colours follow your system's settings.",
    group: "ovra",
    keywords: ["screen reader", "a11y"],
  },
  { id: "about", title: "About", line: "Which Ovra this is.", group: "ovra", keywords: ["version", "licenses"] },
];

/** The pane Settings shows; kept while the view is closed. */
export const settings = $state<{ pane: PaneId }>({ pane: "trust" });

/** The panes there are: the project's only with a project open, Schedules only while they are on. */
export function panesFor(project: boolean, schedules = false): Pane[] {
  return PANES.filter((pane) => (project || pane.group === "ovra") && (!pane.optional || schedules));
}
