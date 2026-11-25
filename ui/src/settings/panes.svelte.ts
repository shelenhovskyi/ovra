// The panes of the Settings view (docs/design/components.md, "Settings"):
// the open project's first, then Ovra's own. Settings reopens on the pane it
// showed last.

export type PaneId = "rules" | "network" | "servers" | "keyboard" | "notifications" | "accessibility" | "about";

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
    id: "rules",
    title: "Rules",
    line: "What agents may do without you: Deny refuses, Ask asks you first, Auto allows. Anything else is asked.",
    group: "project",
    keywords: ["policies", "permissions"],
  },
  {
    id: "network",
    title: "Network",
    line: "What sandboxed agents may reach, and tokens they use without seeing them.",
    group: "project",
    keywords: ["proxy", "token", "hosts", "secrets"],
  },
  {
    id: "servers",
    title: "Servers",
    line: "Dev servers run in the sandbox, where agents can open your app.",
    group: "project",
    keywords: ["dev servers", "ports"],
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
    line: "Every command with a shortcut. Shortcuts follow the key's place, so they work on any layout.",
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
export const settings = $state<{ pane: PaneId }>({ pane: "notifications" });

/** The panes there are: the project's only with a project open. */
export function panesFor(project: boolean): Pane[] {
  return PANES.filter((pane) => project || pane.group === "ovra");
}
