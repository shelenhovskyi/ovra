import { expect, it } from "vitest";
import { review } from "../inbox/fake-review";
import { FakeAgentCore } from "./fake-core";
import { AgentSessions } from "./sessions.svelte";

it("reads review items once more however many review.changed arrive while a reading runs", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Fix sync", { state: "working" });
  const sessions = new AgentSessions(core);
  await sessions.load("p1");
  const lists = () => core.calls.filter((c) => c.method === "review.list").length;
  const before = lists();
  core.reviews = [review("r1", { session: "s1" })];
  for (let i = 0; i < 5; i++) core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
  await sessions.loadReviews("p1");
  // One reading for the first event, one more for the four that came during it.
  expect(lists() - before).toBe(2);
  expect(sessions.reviews.get("s1")?.id).toBe("r1");
});

it("lets panels that open together share one load, while a later load reads again", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Fix sync", { state: "idle" });
  const sessions = new AgentSessions(core);
  const lists = () => core.calls.filter((c) => c.method === "agents.list").length;
  await Promise.all([sessions.show("p1"), sessions.show("p1")]);
  expect(lists()).toBe(1);
  await sessions.show("p1");
  expect(lists()).toBe(2);
  expect(sessions.inProject("p1").map((s) => s.title)).toEqual(["Fix sync"]);
});

it("adds a message once when the transcript's reply overtook its event", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Browse the page", { state: "working" });
  const sessions = new AgentSessions(core);
  await sessions.load("p1");
  const said = { kind: "assistant_text", text: "I will look at the page." } as const;
  // The core added the message, and its reply to agents.transcript went out before the event.
  core.transcripts.set("s1", [said]);
  await sessions.loadTranscript("s1");
  core.emit({ type: "agents.message", data: { session: "s1", message: said, seq: 1 } });
  expect(sessions.transcripts.get("s1")).toEqual([said]);
  // A message after it still comes in.
  core.emit({
    type: "agents.message",
    data: { session: "s1", message: { kind: "assistant_text", text: "Done." }, seq: 2 },
  });
  expect(sessions.transcripts.get("s1")?.map((m) => (m.kind === "assistant_text" ? m.text : m.kind))).toEqual([
    "I will look at the page.",
    "Done.",
  ]);
});
