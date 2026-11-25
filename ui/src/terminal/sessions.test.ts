import { describe, expect, it } from "vitest";
import { CoreError } from "../lib/transport";
import { FakeTerminals } from "./fake-terminals";
import { TerminalSessions } from "./sessions.svelte";

const SIZE = { cols: 80, rows: 24 };

/** A stand-in for the xterm view: `screen` is what it shows. */
function view() {
  const sink = {
    screen: "",
    resets: 0,
    write(data: string) {
      sink.screen += data;
    },
    reset() {
      sink.screen = "";
      sink.resets += 1;
    },
  };
  return sink;
}

function setup() {
  const core = new FakeTerminals();
  const sessions = new TerminalSessions(core);
  const stop = sessions.listen();
  return { core, sessions, stop };
}

/** Lets queued promise callbacks run. */
const settle = () => new Promise((resolve) => setImmediate(resolve));

describe("TerminalSessions", () => {
  it("opens a shell at the measured size and echoes input through the sink", async () => {
    const { core, sessions } = setup();
    const session = sessions.add("p1");
    expect(session.title).toBe("Terminal 1");
    expect(session.state.kind).toBe("starting");

    await sessions.start(session, SIZE);
    expect(session.state).toEqual({ kind: "running", id: "t1" });
    expect(core.open.get("t1")).toMatchObject({ project: "p1", size: SIZE });

    const screen = view();
    session.attach(screen);
    sessions.input(session, "ls\r");
    await settle();
    expect(screen.screen).toBe("ls\r");

    sessions.resize(session, { cols: 100, rows: 30 });
    await settle();
    expect(core.open.get("t1")?.size).toEqual({ cols: 100, rows: 30 });
  });

  it("starts the shell in the tab's folder, also after a restart", async () => {
    const { core, sessions } = setup();
    const session = sessions.add("p1", "src/ui");
    await sessions.start(session, SIZE);
    const fresh = sessions.restart(session);
    if (fresh) await sessions.start(fresh, SIZE);
    await sessions.start(sessions.add("p1"), SIZE);
    const opens = core.calls.filter((call) => call.method === "terminals.open").map((call) => call.params);
    expect(opens).toEqual([
      { project: "p1", size: SIZE, cwd: "src/ui" },
      { project: "p1", size: SIZE, cwd: "src/ui" },
      { project: "p1", size: SIZE },
    ]);
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
    core.output("t1", "hello");
    const screen = view();
    const detach = session.attach(screen);
    expect(screen.screen).toBe("hello");
    detach();
    core.output("t1", " again");
    session.attach(screen);
    expect(screen.screen).toBe("hello again");
  });

  it("marks a tab exited and says how the shell ended", async () => {
    const { core, sessions } = setup();
    const session = sessions.add("p1");
    await sessions.start(session, SIZE);
    const screen = view();
    session.attach(screen);
    core.exit("t1", null);
    expect(session.state).toEqual({ kind: "exited", code: null });
    expect(screen.screen).toContain("Process was killed.");
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

  describe("after missed events", () => {
    async function running() {
      const { core, sessions } = setup();
      const session = sessions.add("p1");
      await sessions.start(session, SIZE);
      const screen = view();
      session.attach(screen);
      return { core, sessions, session, screen };
    }

    it("redraws from the scrollback on info.resync, without duplicates", async () => {
      const { core, screen } = await running();
      core.output("t1", "one ");
      core.output("t1", "two ", true);
      core.output("t1", "three ", true);
      core.emit({ type: "info.resync", data: { skipped: 2 } });
      await settle();
      expect(screen.screen).toBe("one two three ");
      expect(screen.resets).toBe(1);
      core.output("t1", "four");
      expect(screen.screen).toBe("one two three four");
    });

    it("redraws when seq skips, even without a resync", async () => {
      const { core, screen } = await running();
      core.output("t1", "a");
      core.output("t1", "b", true);
      core.output("t1", "c");
      await settle();
      expect(screen.screen).toBe("abc");
      expect(core.methods().filter((m) => m === "terminals.scrollback")).toHaveLength(1);
    });

    it("applies output that arrives during a redraw once, after the scrollback", async () => {
      const { core, screen } = await running();
      let release = () => {};
      core.scrollbackGate = new Promise((resolve) => {
        release = resolve;
      });
      core.output("t1", "a");
      core.emit({ type: "info.resync", data: { skipped: 0 } });
      // Kept by the core after the scrollback answer is built, and seen by the client while waiting.
      const answer = settle();
      core.output("t1", "b");
      release();
      await answer;
      await settle();
      expect(screen.screen).toBe("ab");
    });

    it("keeps the exit notice of an ended shell after a redraw", async () => {
      const { core, screen } = await running();
      core.output("t1", "bye");
      core.exit("t1", 3);
      core.emit({ type: "info.resync", data: { skipped: 0 } });
      await settle();
      expect(screen.screen).toBe("bye\r\nProcess exited with code 3.\r\n");
    });

    it("marks a tab gone when the core no longer knows its terminal", async () => {
      const { core, sessions, session, screen } = await running();
      core.open.clear(); // the core restarted
      core.emit({ type: "info.resync", data: { skipped: 0 } });
      await settle();
      expect(session.state).toEqual({ kind: "lost" });
      expect(screen.screen).toContain("This terminal is gone");
      sessions.close(session);
      expect(core.methods()).not.toContain("terminals.close");
    });
  });
});
