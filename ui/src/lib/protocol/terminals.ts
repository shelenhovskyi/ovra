import type { ProjectId } from "./projects";

export type TerminalId = string;

/** Size in character cells. */
export type TermSize = { cols: number; rows: number };

export type TerminalsRequest =
  | { method: "terminals.open"; params: { project: ProjectId; size: TermSize } }
  | { method: "terminals.input"; params: { id: TerminalId; data: string } }
  | { method: "terminals.resize"; params: { id: TerminalId; size: TermSize } }
  | { method: "terminals.close"; params: { id: TerminalId } };

export type TerminalsResponse = { type: "terminals.opened"; data: { id: TerminalId } };

export type TerminalsEvent =
  /** UTF-8 text; a character split across reads arrives whole in the later chunk. */
  | { type: "terminals.output"; data: { id: TerminalId; data: string } }
  /** `code` is null when the shell was killed by a signal. */
  | { type: "terminals.exited"; data: { id: TerminalId; code: number | null } };
