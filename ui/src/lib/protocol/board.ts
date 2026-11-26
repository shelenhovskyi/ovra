// Mirrors crates/core/src/protocol/board.rs: a task's board, where its agents,
// Ovra and the person leave claims, questions, answers and notes.

import type { TaskId } from "./orchestration";
import type { ProjectId } from "./projects";

/** `b1`, `b2`, ... in the order entries were written, per task. */
export type EntryId = string;

/** Who wrote an entry. The core sets it from the caller, never from text. */
export type Author = { kind: "person" } | { kind: "coordinator" } | { kind: "worker"; name: string } | { kind: "ovra" };

/** Whom a question is for. */
export type Recipient = { kind: "person" } | { kind: "coordinator" } | { kind: "worker"; name: string };

/** A question's answers to pick from (2 to 4), the one the asker recommends and why; absent for free text. */
export type Choices = { options?: string[]; recommended?: number; reason?: string };

export type EntryBody =
  /** A worker takes these paths (relative, globs allowed). */
  | { kind: "claim"; worker: string; paths: string[]; note: string }
  /** The worker gives these paths back. */
  | { kind: "release"; worker: string; paths: string[] }
  | ({ kind: "question"; to: Recipient; text: string } & Choices)
  | { kind: "answer"; question: EntryId; text: string }
  | { kind: "note"; text: string }
  /** Something Ovra saw: an overlap, a review asked for, a stop. */
  | { kind: "ovra"; text: string };

export type BoardEntry = {
  id: EntryId;
  author: Author;
  body: EntryBody;
  /** When it was written, Unix time in milliseconds; missing on older entries. */
  at_ms?: number;
};

export type BoardRequest =
  /** The task's entries from index `since`, oldest first. */
  | { method: "board.list"; params: { task: TaskId; since?: number } }
  /** The person's note. */
  | { method: "board.post"; params: { task: TaskId; text: string } }
  /** The person's answer to a question. */
  | { method: "board.answer"; params: { task: TaskId; question: EntryId; text: string } };

export type BoardResponse =
  | {
      type: "board.entries";
      data: {
        entries: BoardEntry[];
        /** The index after the last entry: pass it as `since` next time. */
        next: number;
        /** Questions for the person that nobody answered yet. */
        open_for_person: EntryId[];
      };
    }
  | { type: "board.entry"; data: BoardEntry };

export type BoardEvent = { type: "board.changed"; data: { project: ProjectId; task: TaskId; entry: BoardEntry } };

export type BoardResponseFor<M extends BoardRequest["method"]> = M extends "board.list"
  ? Extract<BoardResponse, { type: "board.entries" }>
  : Extract<BoardResponse, { type: "board.entry" }>;
