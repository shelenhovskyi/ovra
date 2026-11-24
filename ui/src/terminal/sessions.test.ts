import { describe, expect, it } from "vitest";
import { CoreError } from "../lib/transport";
import { FakeTerminals } from "./fake-terminals";
import { TerminalSessions } from "./sessions.svelte";

const SIZE = { cols: 80, rows: 24 };

function setup() {
  const core = new FakeTerminals();
  const sessions = new TerminalSessions(core);
  const stop = sessions.listen();
  return { core, sessions, stop };
}

/** Lets queued promise callbacks run. */
const settle = () => new Promise((resolve) => setTimeout(resolve));

describe("TerminalSessions", () => {
  it("opens a shell at the measured size and echoes input through the sink", async () => {
    const { core, sessions } = setup();
    const session = sessions.add("p1");
    expect(session.title).toBe("Terminal 1");
    expect(session.state.kind).toBe("starting");

    await sessions.start(session, SIZE);
    expect(session.state).toEqual({ kind: "running", id: "t1" });
    expect(core.open.get("t1")).toEqual({ project: "p1", size: SIZE });

    const written: string[] = [];
    session.attach((data) => written.push(data));
    sessions.input(session, "ls\r");
    await settle();
    expect(written).toEqual(["ls\r"]);

    sessions.resize(session, { cols: 100, rows: 30 });
    await settle();
    expect(core.open.get("t1")?.size).toEqual({ cols: 100, rows: 30 });
  });

  it("sends what was typed while the shell was starting once it runs", async () => {
    const { core, sessions } = setup();
    const session = sessions.add("p1");
    sessions.input(session, "ec");
    sessions.input(session, "ho");
    expect(core.calls).toEqual([]);
    await sessions.start(session, SIZE);
    expect(core.calls.at(-1)).toEqual({ method: "terminals.input", params: { id: "t1", data: "echo" } });
  });

  it("holds output until a view attaches", async () => {
    const { core, sessions } = setup();
    const session = sessions.add("p1");
    await sessions.start(session, SIZE);
    core.emit({ type: "terminals.output", data: { id: "t1", data: "hello" } });
    const written: string[] = [];
    const detach = session.attach((data) => written.push(data));
    expect(written).toEqual(["hello"]);
    detach();
    core.emit({ type: "terminals.output", data: { id: "t1", data: " again" } });
    session.attach((data) => written.push(data));
    expect(written).toEqual(["hello", " again"]);
  });

  it("marks a tab exited and says how the shell ended", async () => {
    const { core, sessions } = setup();
    const session = sessions.add("p1");
    await sessions.start(session, SIZE);
    const written: string[] = [];
    session.attach((data) => written.push(data));
    core.emit({ type: "terminals.exited", data: { id: "t1", code: null } });
    expect(session.state).toEqual({ kind: "exited", code: null });
    expect(written.join("")).toContain("Process was killed.");
    sessions.input(session, "x");
    expect(core.methods()).not.toContain("terminals.input");
  });

  it("shows why a shell could not start, without the error code", async () => {
    const { core, sessions } = setup();
    core.openError = new CoreError({ code: "unavailable", message: "terminals are not implemented yet" });
    const session = sessions.add("p1");
    await sessions.start(session, SIZE);
    expect(session.state).toEqual({ kind: "failed", message: "terminals are not implemented yet" });
  });

  it("keeps tabs per project and activates a neighbour on close", async () => {
    const { core, sessions } = setup();
    const a = sessions.add("p1");
    const b = sessions.add("p1");
    const other = sessions.add("p2");
    expect([a.title, b.title, other.title]).toEqual(["Terminal 1", "Terminal 2", "Terminal 1"]);
    expect(sessions.of("p1")).toEqual([a, b]);
    expect(sessions.active("p1")).toBe(b);

    await sessions.start(b, SIZE);
    sessions.close(b);
    await settle();
    expect(sessions.active("p1")).toBe(a);
    expect(core.methods()).toContain("terminals.close");
    expect(core.open.size).toBe(0);
    expect(sessions.active("p2")).toBe(other);

    sessions.close(a);
    expect(sessions.active("p1")).toBeUndefined();
    expect(sessions.add("p1").title).toBe("Terminal 1");
  });

  it("ends a shell whose tab was closed while it was starting", async () => {
    const { core, sessions } = setup();
    let release = () => {};
    core.openGate = new Promise((resolve) => {
      release = resolve;
    });
    const session = sessions.add("p1");
    const started = sessions.start(session, SIZE);
    sessions.close(session);
    release();
    await started;
    expect(core.methods()).toEqual(["terminals.open", "terminals.close"]);
    expect(core.open.size).toBe(0);
  });
});
