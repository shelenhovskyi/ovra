import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import type { CoreEvent, PermissionRequest, Request, ReviewItem, SessionInfo } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import App from "./App.svelte";
import Approval from "./Approval.svelte";
import { memoryStore } from "./identity";
import { finished, forReview, needsYou, Phone, working } from "./phone.svelte";
import { pushSupport } from "./push";
import { parseRoute } from "./route";

const session = (id: string, state: SessionInfo["status"]["state"]): SessionInfo =>
  ({
    id,
    project: "p1",
    agent: "claude",
    mode: "direct",
    status: state === "failed" ? { state, message: "x" } : { state },
    title: `Task ${id}`,
  }) as SessionInfo;

const ask = (id: string, on: string): PermissionRequest => ({
  id,
  session: on,
  project: "p1",
  tool: "Bash",
  input: { command: "ls -la" },
  call: { kind: "execute", command: "ls -la" },
  title: null,
});

function fake(sessions: SessionInfo[], pending: PermissionRequest[] = []) {
  const sent: Request[] = [];
  const listeners = new Set<(e: CoreEvent) => void>();
  const transport: Transport = {
    call: ((request: Request) => {
      sent.push(request);
      const data = (() => {
        switch (request.method) {
          case "projects.list":
            return [{ id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true }];
          case "power.get":
          case "power.set_mode":
            return { mode: request.method === "power.set_mode" ? request.params.mode : "off", awake: false };
          case "agents.list":
            return sessions;
          case "permissions.pending":
            return pending;
          case "agents.capacity":
            return { working: 1, limit: 3, queued: 0, research_working: 0, research_limit: 2, waiting: [] };
          case "agents.transcript":
            return [{ kind: "user_text", text: "Fix it", from: "person" }];
          default:
            return [];
        }
      })();
      return Promise.resolve({ type: request.method, data });
    }) as Transport["call"],
    onEvent: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const emit = (event: CoreEvent) => {
    for (const listener of listeners) listener(event);
  };
  return { transport, sent, emit };
}

let app: Record<string, unknown> | null = null;
afterEach(() => {
  if (app) unmount(app);
  app = null;
  vi.useRealTimers();
});

it("loads every project and sorts sessions into needs you, working and finished", async () => {
  const { transport, sent, emit } = fake(
    [session("a", "waiting_for_approval"), session("b", "working"), session("c", "idle")],
    [ask("r1", "a")],
  );
  const phone = new Phone(transport);
  await vi.waitFor(() => expect(phone.loaded).toBe(true));
  await vi.waitFor(() => expect(phone.approvals.pending).toHaveLength(1));
  expect(needsYou(phone).map((s) => s.id)).toEqual(["a"]);
  expect(working(phone).map((s) => s.id)).toEqual(["b"]);
  expect(finished(phone).map((s) => s.id)).toEqual(["c"]);
  expect(phone.capacity.get("p1")?.limit).toBe(3);
  expect(sent.map((r) => r.method)).not.toContain("agents.start");

  emit({ type: "agents.status", data: { session: "b", project: "p1", status: { state: "idle" } } });
  expect(finished(phone).map((s) => s.id)).toEqual(["b", "c"]);
  emit({ type: "power.changed", data: { mode: "always", awake: true } });
  expect(phone.power?.mode).toBe("always");

  await phone.loadTranscript("a");
  emit({ type: "agents.message", data: { session: "a", message: { kind: "assistant_text", text: "On it" } } });
  expect(phone.transcripts.get("a")).toHaveLength(2);

  await phone.reply("a", "go on");
  await phone.interrupt("a");
  await phone.setMode("while_agents_work");
  expect(sent.slice(-3)).toEqual([
    { method: "agents.send", params: { session: "a", text: "go on" } },
    { method: "agents.interrupt", params: { session: "a" } },
    { method: "power.set_mode", params: { mode: "while_agents_work" } },
  ]);
});

it("arms Allow only after 500 ms and never sends Always allow", async () => {
  vi.useFakeTimers();
  const { transport, sent } = fake([session("a", "waiting_for_approval")], [ask("r1", "a")]);
  const phone = new Phone(transport);
  await vi.waitFor(() => expect(phone.approvals.pending).toHaveLength(1));
  const target = document.createElement("div");
  document.body.append(target);
  app = mount(Approval, { target, props: { phone, id: "r1" } });
  flushSync();
  expect(target.querySelector("h2")?.textContent).toBe("Run ls -la");
  const allow = [...target.querySelectorAll("button")].find((b) => b.textContent === "Allow");
  allow?.click();
  await vi.advanceTimersByTimeAsync(0);
  expect(sent.some((r) => r.method === "permissions.answer")).toBe(false);
  await vi.advanceTimersByTimeAsync(500);
  flushSync();
  allow?.click();
  await vi.waitFor(() =>
    expect(sent.at(-1)).toEqual({ method: "permissions.answer", params: { id: "r1", decision: "allow" } }),
  );
  await vi.waitFor(() => expect(location.hash).toBe("#/"));
  target.remove();
});

it("reads routes and where push works", () => {
  expect(parseRoute("#/agent/a%2Fb")).toEqual({ screen: "agent", session: "a/b" });
  expect(parseRoute("#/approval/r1")).toEqual({ screen: "approval", id: "r1" });
  expect(parseRoute("#pair=x")).toEqual({ screen: "home" });
  const env = { serviceWorker: true, pushManager: true, notification: true, standalone: false, appleMobile: true };
  expect(pushSupport(env)).toBe("ready");
  expect(pushSupport({ ...env, pushManager: false })).toBe("add_to_home_screen");
  expect(pushSupport({ ...env, pushManager: false, appleMobile: false })).toBe("unsupported");
});

it("lists for review only open work an agent asked to have reviewed", async () => {
  const phone = new Phone(fake([]).transport);
  await vi.waitFor(() => expect(phone.loaded).toBe(true));
  const item = (id: string, state: ReviewItem["state"], summary: string | null) =>
    ({ id, session: "a", project: "p1", state, summary, files: [] }) as unknown as ReviewItem;
  phone.reviews = [
    item("r1", "open", "Fixed the tests"),
    item("r2", "open", null),
    item("r3", "approved", "Kept already"),
  ];
  expect(forReview(phone).map((r) => r.id)).toEqual(["r1"]);
});

it("the Home Screen app pairs from a pasted link; Safari on an iPhone says to do that first", async () => {
  const origin = location.origin;
  const target = document.body.appendChild(document.createElement("div"));
  const homeScreen = { standalone: true, appleMobile: true };
  app = mount(App, { target, props: { store: memoryStore(), origin, homeScreen } });
  await vi.waitFor(() => expect(target.querySelector("#unpaired")).not.toBeNull());
  const input = target.querySelector<HTMLInputElement>("#pair-link");
  const form = input?.form;
  if (!input || !form) throw new Error("no paste form");
  input.value = "https://elsewhere.example/#pair=AbCdEfGhIjKlMnOpQrStUv";
  input.dispatchEvent(new Event("input"));
  form.requestSubmit();
  flushSync();
  expect(target.querySelector('[role="alert"]')?.textContent).toContain("not a pairing link");
  input.value = `${origin}/#pair=AbCdEfGhIjKlMnOpQrStUv`;
  input.dispatchEvent(new Event("input"));
  form.requestSubmit();
  flushSync();
  expect(target.querySelector("#pair-title")).not.toBeNull();
  expect(target.querySelector('[data-testid="home-screen-first"]')).toBeNull();
  unmount(app);

  history.replaceState(null, "", "/#pair=AbCdEfGhIjKlMnOpQrStUv");
  const safari = { standalone: false, appleMobile: true };
  app = mount(App, { target, props: { store: memoryStore(), origin, homeScreen: safari } });
  await vi.waitFor(() => expect(target.querySelector('[data-testid="home-screen-first"]')).not.toBeNull());
  expect(target.querySelector("#pair-link")).toBeNull();
  target.remove();
});
