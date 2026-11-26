// Each task's board (`board.*`): what its workers, coordinator, Ovra and the
// person wrote there. One instance per transport, shared by the task list
// (its "Answer" chip) and the task's Board tab.

import { SvelteMap } from "svelte/reactivity";
import type { BoardEntry, CoreEvent, EntryId, TaskId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export class TaskBoards {
  /** Each watched task's entries, oldest first. */
  readonly entries = new SvelteMap<TaskId, BoardEntry[]>();
  private readonly watched = new Set<TaskId>();

  constructor(private readonly transport: Transport) {
    transport.onEvent((event) => this.apply(event));
    let wasOpen = false;
    transport.onStatus?.((status) => {
      if (status !== "open") return;
      // After a reconnect events may have been missed: load again.
      if (wasOpen) this.reload();
      wasOpen = true;
    });
  }

  /** Loads `task`'s board once and keeps it up to date from then on. */
  watch(task: TaskId): void {
    if (this.watched.has(task)) return;
    this.watched.add(task);
    void this.load(task);
  }

  /** Questions for the person on `task`'s board that nobody answered yet, oldest first. */
  openForPerson(task: TaskId): BoardEntry[] {
    const entries = this.entries.get(task) ?? [];
    const answered = new Set(entries.flatMap((e) => (e.body.kind === "answer" ? [e.body.question] : [])));
    return entries.filter((e) => e.body.kind === "question" && e.body.to.kind === "person" && !answered.has(e.id));
  }

  async post(task: TaskId, text: string): Promise<void> {
    const { data } = await this.transport.call({ method: "board.post", params: { task, text } });
    this.add(task, data);
  }

  async answer(task: TaskId, question: EntryId, text: string): Promise<void> {
    const { data } = await this.transport.call({ method: "board.answer", params: { task, question, text } });
    this.add(task, data);
  }

  private async load(task: TaskId): Promise<void> {
    try {
      const { data } = await this.transport.call({ method: "board.list", params: { task } });
      // Entries that came as events while it loaded are in the list too.
      this.entries.set(task, data.entries);
    } catch {
      // A core without boards, or a task that is gone: no board to show.
      this.watched.delete(task);
    }
  }

  private reload(): void {
    for (const task of this.watched) void this.load(task);
  }

  private apply(event: CoreEvent): void {
    if (event.type === "info.resync") this.reload();
    else if (event.type === "board.changed" && this.watched.has(event.data.task))
      this.add(event.data.task, event.data.entry);
  }

  /** Adds `entry` unless it is there already (the reply to the person's own write and its event both bring it). */
  private add(task: TaskId, entry: BoardEntry): void {
    const entries = this.entries.get(task);
    if (!entries) return;
    if (!entries.some((e) => e.id === entry.id)) this.entries.set(task, [...entries, entry]);
  }
}

const instances = new WeakMap<Transport, TaskBoards>();

export function taskBoards(transport: Transport): TaskBoards {
  let boards = instances.get(transport);
  if (!boards) {
    boards = new TaskBoards(transport);
    instances.set(transport, boards);
  }
  return boards;
}
