// Test-only fake of the core's `browser.*`: records calls, opens pages, and
// answers `browser.pick` with `element(x, y, up)`. Tests send frames, page
// lists and navigation asks themselves with `emit`.

import type {
  BrowserRequest,
  CoreEvent,
  PageInfo,
  Pick,
  ProtocolError,
  Rect,
  Request,
  ResponseFor,
  SessionId,
  SessionInfo,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

type ParamsOf<M extends BrowserRequest["method"]> = Extract<BrowserRequest, { method: M }>["params"];

/** A pick of a `tag` element with box `rect`. */
export function fakePick(tag: string, rect: Rect, over: Partial<Pick> = {}): Pick {
  return {
    pick_id: `pk-${tag}`,
    url: "http://localhost:5173/",
    selector: `body > ${tag}`,
    locator: `${tag} "${tag}"`,
    tag,
    text: "",
    html: `<${tag}></${tag}>`,
    rect,
    ...over,
  };
}

export class FakeBrowserCore implements Transport {
  calls: Request[] = [];
  sessions: SessionInfo[] = [session("s1")];
  /** Errors to answer with, by method. */
  refuse = new Map<string, ProtocolError>();
  /** What `browser.pick` answers. */
  element: (x: number, y: number, up: number) => Pick = (_x, _y, up) =>
    fakePick(up === 0 ? "button" : `div${up}`, { x: 10 * up, y: 10 * up, width: 100, height: 40 });
  private nextPage = 1;
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    const refused = this.refuse.get(request.method);
    if (refused) throw new CoreError(refused);
    switch (request.method) {
      case "agents.list":
        return { type: "agents.list", data: this.sessions } as ResponseFor<R>;
      case "orchestration.list":
        return { type: "orchestration.list", data: [] } as unknown as ResponseFor<R>;
      case "browser.open": {
        const { session, url } = request.params;
        const data: PageInfo = { id: `new${this.nextPage++}`, session, url, title: "" };
        return { type: "browser.page", data } as ResponseFor<R>;
      }
      case "browser.pick": {
        const { x, y, up } = request.params;
        return { type: "browser.picked", data: this.element(x, y, up ?? 0) } as ResponseFor<R>;
      }
      case "browser.close":
      case "browser.screencast":
      case "browser.input":
      case "browser.attach":
      case "browser.answer_navigation":
      case "agents.send":
        return { type: "ok" } as ResponseFor<R>;
      default:
        throw new Error(`unexpected ${request.method}`);
    }
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  /** The params of each call of one `browser.*` method. */
  sent<M extends BrowserRequest["method"]>(method: M): ParamsOf<M>[] {
    return this.calls.filter((c) => c.method === method).map((c) => (c as { params: unknown }).params as ParamsOf<M>);
  }

  /** The session's pages change, as `browser.pages` says. */
  pages(session: SessionId, pages: PageInfo[]): void {
    this.emit({ type: "browser.pages", data: { session, pages } });
  }
}

function session(id: SessionId): SessionInfo {
  return {
    id,
    project: "p1",
    agent: "claude",
    mode: "direct",
    status: { state: "idle" },
    title: "Fix the header",
    usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
    resumable: false,
    billing: null,
    sandbox: null,
    sandbox_reported: false,
  };
}
