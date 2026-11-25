// The panes of the Settings view (docs/design/components.md, "Settings"):
// the open project's first, then Ovra's own. Settings reopens on the pane it
// showed last.

export type PaneId =
  | "trust"
  | "rules"
  | "network"
  | "servers"
  | "editor"
  | "claude"
  | "keyboard"
  | "notifications"
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
    line: "Hosts sandboxed agents may reach, and tokens the proxy adds for them.",
    group: "project",
    keywords: ["proxy", "token", "hosts", "secrets"],
  },
  {
    id: "servers",
    title: "Servers",
    line: "Commands that start your app, so you and agents can open it.",
    group: "project",
    keywords: ["dev servers", "ports"],
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
    title: "Claude",
    line: "The Claude Code on this computer that runs your tasks, and who pays for them.",
    group: "ovra",
    keywords: ["account", "billing", "path", "sign in", "agents at once", "parallel", "limit"],
  },
  {
    id: "notifications",
    title: "Notifications",
    line: "How Ovra tells you that an agent needs you.",
    group: "ovra",
    keywords: ["alerts", "toasts"],
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

/** The panes there are: the project's only with a project open. */
export function panesFor(project: boolean): Pane[] {
  return PANES.filter((pane) => project || pane.group === "ovra");
}
