// Mirrors crates/core/src/protocol/context.rs: what the person adds to a
// task or reply with `@` (docs/specs/task-context.md).

import type { SessionId } from "./agents";
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

export type ContextRequest =
  | {
      /** `forbidden` for a path outside the project, `bad_request` for one too large. */
      method: "context.check";
      /** `session`: the task a reply goes to; a page item needs it. */
      params: { project: ProjectId; session?: SessionId; items: ContextItem[] };
    }
  | {
      /**
       * Saves a pasted or dropped file (`data` in base64, 20 MiB at most) as a new
       * file of the attachments folder, named `name` or like it. Answers `context.attached`.
       */
      method: "context.attach";
      params: { project: ProjectId; name: string; data: string };
    }
  /** Makes the attachments folder, for a drop the app copies there. Answers `context.attached`. */
  | { method: "context.attachments"; params: { project: ProjectId } };

export type ContextResponse =
  /** One note per item, in order: null when it can go as it is. */
  | { type: "context.checked"; data: { notes: (ContextWarning | null)[] } }
  /** The saved file's path, or the attachments folder's. */
  | { type: "context.attached"; data: { path: string } };

export type ContextResponseFor<M extends ContextRequest["method"]> = M extends "context.check"
  ? Extract<ContextResponse, { type: "context.checked" }>
  : Extract<ContextResponse, { type: "context.attached" }>;
