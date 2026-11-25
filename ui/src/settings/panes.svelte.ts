// The panes of the Settings view (docs/design/components.md, "Settings"):
// the open project's first, then Ovra's own. Settings reopens on the pane it
// showed last.

export type PaneId = "keyboard" | "notifications" | "accessibility" | "about";

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
