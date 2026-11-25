// The agent browser as the UI knows it: each session's pages and the
// navigations waiting for the person, kept current from `browser.*` events.
// One instance per transport (`sessionBrowsers(transport)`), shared by the
// header button and the pane.

import { SvelteMap } from "svelte/reactivity";
import type { CoreEvent, PageId, PageInfo, SessionId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import type { Refusal } from "./refusal";

/** A page wants an origin that is not one of the session's servers. */
export type NavigationAsk = { page: PageId; session: SessionId; origin: string };

export class SessionBrowsers {
  /** Open pages by session, in the order the core lists them. */
  readonly pages = new SvelteMap<SessionId, PageInfo[]>();
  /** Navigations waiting for an answer, oldest first. */
  asks = $state<NavigationAsk[]>([]);
  /** Why the browser cannot run, by session: from `browser.status`, or from a call it refused. */
  readonly refusals = new SvelteMap<SessionId, Refusal>();
  /** Counts the `browser.status` asks per session, so only the latest answer counts. */
  private readonly asked = new Map<SessionId, number>();
  /** Whether the pane is shown. */
  shown = $state(false);

  constructor(private readonly transport: Transport) {
    transport.onEvent((event) => this.apply(event));
  }

  of(session: SessionId): PageInfo[] {
    return this.pages.get(session) ?? [];
  }

  /** Opens `url` in a new page of the session's browser; null when the browser cannot run for it. */
  async open(session: SessionId, url: string): Promise<PageInfo | null> {
    const data = await this.guard(session, async () => {
      const { data } = await this.transport.call({ method: "browser.open", params: { session, url } });
      return data;
    });
    if (!data) return null;
    // The reply may come before or after `browser.pages`.
    const known = this.of(session);
    if (!known.some((p) => p.id === data.id)) this.pages.set(session, [...known, data]);
    return data;
  }

  async close(page: PageInfo): Promise<void> {
    await this.guard(page.session, () => this.transport.call({ method: "browser.close", params: { page: page.id } }));
    this.pages.set(
      page.session,
      this.of(page.session).filter((p) => p.id !== page.id),
    );
  }

  async answer(ask: NavigationAsk, allow: boolean): Promise<void> {
    const { page, origin } = ask;
    await this.guard(ask.session, () =>
      this.transport.call({ method: "browser.answer_navigation", params: { page, origin, allow } }),
    );
    this.asks = this.asks.filter((a) => a !== ask);
  }

  /** Asks the core whether the session's browser can run, and keeps why not; `keep` keeps a refusal it no longer sees. */
  async check(session: SessionId, keep = false): Promise<void> {
    const mine = (this.asked.get(session) ?? 0) + 1;
    this.asked.set(session, mine);
    try {
      const { data } = await this.transport.call({ method: "browser.status", params: { session } });
      // A later ask answers for now; this one is out of date.
      if (this.asked.get(session) !== mine) return;
      if (data.state === "refused") this.refusals.set(session, { code: data.code, reason: data.reason });
      else if (!keep) this.refusals.delete(session);
    } catch {
      // A lost connection: the next call says why, if it fails.
    }
  }

  /** Runs a call for `session`; `unavailable` is remembered with its code and answers null, other errors are thrown. */
  async guard<T>(session: SessionId, call: () => Promise<T>): Promise<T | null> {
    try {
      return await call();
    } catch (err) {
      if (err instanceof CoreError && err.code === "unavailable") {
        this.refusals.set(session, { code: null, reason: err.error.message });
        await this.check(session, true);
        return null;
      }
      throw err;
    }
  }

  private apply(event: CoreEvent): void {
    if (event.type === "browser.pages") {
      const { session, pages } = event.data;
      this.pages.set(session, pages);
      // A closed page asks nothing any more.
      this.asks = this.asks.filter((a) => a.session !== session || pages.some((p) => p.id === a.page));
    } else if (event.type === "browser.navigation_asked") {
      const ask = event.data;
      if (!this.asks.some((a) => a.page === ask.page && a.origin === ask.origin)) this.asks = [...this.asks, ask];
    }
  }
}

const instances = new WeakMap<Transport, SessionBrowsers>();

export function sessionBrowsers(transport: Transport): SessionBrowsers {
  let browsers = instances.get(transport);
  if (!browsers) {
    browsers = new SessionBrowsers(transport);
    instances.set(transport, browsers);
  }
  return browsers;
}
