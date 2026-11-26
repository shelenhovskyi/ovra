// Mirrors crates/core/src/protocol/context.rs: what the person adds to a
// task or reply with `@` (docs/specs/task-context.md).

import type { PageId } from "./browser";
import type { ProjectId } from "./projects";

/** Lines are 1-based. Files and folders are referenced; the agent reads them itself. */
export type ContextWhat =
  | { kind: "file"; path: string }
  | { kind: "folder"; path: string }
  | { kind: "selection"; path: string; from_line: number; to_line: number; text: string }
  | { kind: "problem"; path: string; line: number; severity: string; message: string; source?: string | null }
  | { kind: "terminal"; title: string; text: string }
  | { kind: "page"; page: PageId };

/** `confirmed`: the person chose Include on its warning. */
export type ContextItem = ContextWhat & { confirmed?: boolean };

/** Why an item needs the person's Include before it is sent. */
export type ContextWarning = "protected" | "secret_file" | "secret_text";

export type ContextRequest = {
  /** `forbidden` for a path outside the project, `bad_request` for one too large. */
  method: "context.check";
  params: { project: ProjectId; items: ContextItem[] };
};

/** One note per item, in order: null when it can go as it is. */
export type ContextResponse = { type: "context.checked"; data: { notes: (ContextWarning | null)[] } };
