// The agent browser as the UI knows it: each session's pages and the
// navigations waiting for the person, kept current from `browser.*` events.
// One instance per transport (`sessionBrowsers(transport)`), shared by the
// header button and the pane.

import { SvelteMap } from "svelte/reactivity";
import type { CoreEvent, PageId, PageInfo, SessionId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

/** A page wants an origin that is not one of the session's servers. */
export type NavigationAsk = { page: PageId; session: SessionId; origin: string };

export class SessionBrowsers {
  /** Open pages by session, in the order the core lists them. */
  readonly pages = new SvelteMap<SessionId, PageInfo[]>();
  /** Navigations waiting for an answer, oldest first. */
  asks = $state<NavigationAsk[]>([]);
  /** The core's reason when the browser cannot run here; set by the first call it refuses. */
  unavailable = $state<string | null>(null);
  /** Whether the pane is shown. */
  shown = $state(false);

  constructor(private readonly transport: Transport) {
    transport.onEvent((event) => this.apply(event));
  }

  of(session: SessionId): PageInfo[] {
    return this.pages.get(session) ?? [];
  }

  /** Opens `url` in a new page of the session's browser; null when the browser cannot run here. */
  async open(session: SessionId, url: string): Promise<PageInfo | null> {
    const data = await this.guard(async () => {
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
    await this.guard(() => this.transport.call({ method: "browser.close", params: { page: page.id } }));
    this.pages.set(
      page.session,
      this.of(page.session).filter((p) => p.id !== page.id),
    );
  }

  async answer(ask: NavigationAsk, allow: boolean): Promise<void> {
    const { page, origin } = ask;
    await this.guard(() =>
      this.transport.call({ method: "browser.answer_navigation", params: { page, origin, allow } }),
    );
    this.asks = this.asks.filter((a) => a !== ask);
  }

  /** Runs a call; `unavailable` is remembered and answers null, other errors are thrown. */
  async guard<T>(call: () => Promise<T>): Promise<T | null> {
    try {
      return await call();
    } catch (err) {
      if (err instanceof CoreError && err.code === "unavailable") {
        this.unavailable = err.error.message;
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
