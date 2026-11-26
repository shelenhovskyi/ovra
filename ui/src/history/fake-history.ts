// Test-only fake of the core's `agents.history` and `projects.list`: filters
// `entries` by project and by words of the title or snippet, as the core does.

import type { HistoryEntry, Project, Request, ResponseFor, SessionInfo } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export function entry(
  id: string,
  active: number | null,
  extra: Partial<SessionInfo> = {},
  snippet?: string,
): HistoryEntry {
  const from = snippet?.indexOf("[") ?? -1;
  const to = snippet?.indexOf("]") ?? -1;
  return {
    session: {
      id,
      project: "p1",
      agent: "claude",
      mode: "direct",
      status: { state: "stopped" },
      title: id,
      usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
      resumable: true,
      billing: null,
      sandbox: null,
      sandbox_reported: false,
      account: null,
      active_ms: active,
      ...extra,
    },
    snippet: snippet ? { text: snippet.replace(/[[\]]/g, ""), from, to: to - 1 } : null,
  };
}

const project = (id: string, name: string): Project => ({
  id,
  name,
  root: `/${name}`,
  last_opened_ms: null,
  trusted: false,
});

export class FakeHistoryCore implements Transport {
  calls: Request[] = [];
  entries: HistoryEntry[] = [];
  projects: Project[] = [project("p1", "demo"), project("p2", "site")];

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    if (request.method === "projects.list") return { type: "projects.list", data: this.projects } as ResponseFor<R>;
    if (request.method !== "agents.history") throw new Error(`unexpected ${request.method}`);
    const words = request.params.query.toLowerCase().split(/\s+/).filter(Boolean);
    const data = this.entries.filter(
      (e) =>
        (!request.params.project || e.session.project === request.params.project) &&
        words.every((w) => `${e.session.title} ${e.snippet?.text ?? ""}`.toLowerCase().includes(w)),
    );
    return { type: "agents.history", data } as ResponseFor<R>;
  }

  onEvent(): () => void {
    return () => {};
  }
}
