import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentMessage } from "../lib/protocol";
import { FakeAgentCore } from "./fake-core";
import { AgentSessions } from "./sessions.svelte";
import Transcript from "./Transcript.svelte";

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

describe("LiveActivity", () => {
  it("keeps streamed text until its message comes, and drops it when the turn ends", () => {
    const core = new FakeAgentCore();
    const sessions = new AgentSessions(core);
    const delta = (text: string) => core.emit({ type: "agents.text_delta", data: { session: "s1", text } });
    delta("Read");
    delta("ing.");
    expect(sessions.partial.get("s1")).toBe("Reading.");
    core.message("s1", { kind: "assistant_text", text: "Reading.", parent: "t0" });
    expect(sessions.partial.get("s1"), "a subagent's message is another one").toBe("Reading.");
    core.message("s1", { kind: "assistant_text", text: "Reading." });
    expect(sessions.partial.has("s1")).toBe(false);
    delta("Half");
    core.status("s1", { state: "idle" });
    expect(sessions.partial.has("s1")).toBe(false);
  });

  it("shows what a working session does and for how long, at the end of the transcript", () => {
    vi.useFakeTimers();
    const target = document.createElement("div");
    document.body.append(target);
    const props = $state({
      messages: [{ kind: "user_text", text: "Go", from: "person" }] as AgentMessage[],
      status: { state: "working" } as const,
      partial: "",
    });
    const app = mount(Transcript, { target, props });
    flushSync();
    const live = () => target.querySelector("[data-testid=live-activity]")?.textContent?.replace(/\s+/g, " ").trim();
    expect(live()).toBe("Thinking");
    vi.advanceTimersByTime(4000);
    flushSync();
    expect(live()).toBe("Thinking · 4s");
    const call = { kind: "execute", command: "npm test" } as const;
    props.messages = [
      ...props.messages,
      { kind: "tool_use", id: "t1", name: "Bash", input: { command: "npm test" }, call },
    ];
    flushSync();
    // A new step counts from zero.
    expect(live()).toBe("Running: npm test");
    props.partial = "All tests";
    flushSync();
    expect(target.querySelector("[data-testid=streaming-text]")?.textContent).toContain("All tests");
    // The last item is the live row.
    expect(target.querySelector("[data-testid=transcript] > ol > li:last-child")?.classList.contains("live")).toBe(
      true,
    );
    unmount(app);
  });
});
