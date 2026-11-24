import type { ProjectId } from "./projects";

export type TerminalId = string;

/** Size in character cells. */
export type TermSize = { cols: number; rows: number };

export type TerminalsRequest =
  | { method: "terminals.open"; params: { project: ProjectId; size: TermSize } }
  | { method: "terminals.input"; params: { id: TerminalId; data: string } }
  | { method: "terminals.resize"; params: { id: TerminalId; size: TermSize } }
  | { method: "terminals.close"; params: { id: TerminalId } }
  /** The kept tail of the output; an ended terminal answers until it is closed. */
  | { method: "terminals.scrollback"; params: { id: TerminalId } };

export type TerminalsResponse =
  | { type: "terminals.opened"; data: { id: TerminalId } }
  /** About the last 1 MiB of output, up to and including output event `seq` (0: none yet). */
  | { type: "terminals.scrollback"; data: { data: string; seq: number } };

export type TerminalsEvent =
  /**
   * UTF-8 text; a character split across reads arrives whole in the later chunk.
   * `seq` counts one terminal's output events from 1, so a gap shows missed output.
   */
  | { type: "terminals.output"; data: { id: TerminalId; data: string; seq: number } }
  /** `code` is null when the shell was killed by a signal. */
  | { type: "terminals.exited"; data: { id: TerminalId; code: number | null } };
