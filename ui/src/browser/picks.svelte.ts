// Elements the person picked and has not sent yet, per session. The browser
// pane adds them; the session's message box shows them as chips and attaches
// them when the message is sent (`attach`, before `agents.send`, since the
// session's next message carries them). One instance per transport.

import { SvelteMap } from "svelte/reactivity";
import type { PageId, Pick, SessionId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

/** A pick waiting to be sent: its page and the page's scroll when it was picked. */
export type PendingPick = { page: PageId; pick: Pick; scroll: { x: number; y: number } };

export class BrowserPicks {
  readonly bySession = new SvelteMap<SessionId, PendingPick[]>();

  constructor(private readonly transport: Transport) {}

  of(session: SessionId): PendingPick[] {
    return this.bySession.get(session) ?? [];
  }

  /** Adds a pick; picking the same element again keeps the first one and its note. */
  add(session: SessionId, page: PageId, pick: Pick, scroll = { x: 0, y: 0 }): void {
    const known = this.of(session);
    if (known.some((p) => p.page === page && p.pick.selector === pick.selector)) return;
    this.bySession.set(session, [...known, { page, pick: { ...pick, note: pick.note ?? "" }, scroll }]);
  }

  remove(session: SessionId, pickId: string): void {
    this.keep(session, (p) => p.pick.pick_id !== pickId);
  }

  note(session: SessionId, pickId: string, note: string): void {
    this.bySession.set(
      session,
      this.of(session).map((p) => (p.pick.pick_id === pickId ? { ...p, pick: { ...p.pick, note } } : p)),
    );
  }

  /** Hands the session's picks to the core, one call per page, and forgets them. */
  async attach(session: SessionId): Promise<void> {
    const byPage = new Map<PageId, Pick[]>();
    for (const { page, pick } of this.of(session)) byPage.set(page, [...(byPage.get(page) ?? []), pick]);
    for (const [page, picks] of byPage) {
      await this.transport.call({ method: "browser.attach", params: { page, picks } });
      // A failure on a later page must not attach this one twice.
      this.keep(session, (p) => p.page !== page);
    }
  }

  private keep(session: SessionId, which: (pick: PendingPick) => boolean): void {
    const rest = this.of(session).filter(which);
    if (rest.length > 0) this.bySession.set(session, rest);
    else this.bySession.delete(session);
  }
}

/** A pick's name for people: its locator, else its tag and id. */
export function pickLabel(pick: Pick): string {
  return pick.locator || (pick.id ? `${pick.tag}#${pick.id}` : pick.tag);
}

const instances = new WeakMap<Transport, BrowserPicks>();

export function browserPicks(transport: Transport): BrowserPicks {
  let picks = instances.get(transport);
  if (!picks) {
    picks = new BrowserPicks(transport);
    instances.set(transport, picks);
  }
  return picks;
}
