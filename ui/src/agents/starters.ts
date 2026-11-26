// Ready-made first tasks for an empty agents screen: one press starts one,
// with a prompt written so a newcomer gets a useful result.

import type { TaskMode } from "./task-mode";

export type Starter = {
  id: string;
  label: string;
  /** One line under the label: what the agent will do. */
  what: string;
  mode: TaskMode;
  prompt: string;
};

export const STARTERS: readonly Starter[] = [
  {
    id: "explain",
    label: "Explain this project",
    what: "Reads the code and changes nothing.",
    mode: "research",
    prompt:
      "Explain this project to someone who is new to it: what it does, how the code is organized (the main " +
      "folders and what lives in each), how to build, run and test it, and where to start reading. Keep it " +
      "short, point to files as path:line, and do not change any files.",
  },
  {
    id: "tests",
    label: "Fix failing tests",
    what: "Runs the tests and fixes what fails.",
    mode: "here",
    prompt:
      "Find how this project runs its tests (README, package.json, Makefile, justfile, CI config), run them " +
      "and fix what fails. Change the code, not the tests, unless a test is clearly wrong, and say so when you " +
      "change one. If every test passes, say so and change nothing.",
  },
  {
    id: "readme",
    label: "Write a README",
    what: "Describes what the project is and how to use it.",
    mode: "here",
    prompt:
      "Write a README.md for this project, or improve the one it has: what the project is, how to install, " +
      "run and test it, and how the code is organized. Base every statement on the code and config, never " +
      "invent features, and keep it short.",
  },
];

/** How a starter runs: research is off or full, so the explainer reads the project as one agent here instead. */
export function starterMode(starter: Starter, researchOpen: boolean): TaskMode {
  return starter.mode === "research" && !researchOpen ? "here" : starter.mode;
}
