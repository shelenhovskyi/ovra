import { describe, expect, it } from "vitest";
import { CoreError } from "../lib/transport";
import { FakeTerminals } from "./fake-terminals";
import { type SavedTabs, type TabStore, TerminalSessions } from "./sessions.svelte";

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

  describe("after a reload", () => {
    /** A tab store in memory, as the page's storage keeps it. */
    function memory(): TabStore & { saved: Map<string, SavedTabs> } {
      const saved = new Map<string, SavedTabs>();
      return {
        saved,
        load: (project) => saved.get(project) ?? null,
        save: (project, tabs) => (tabs ? saved.set(project, structuredClone(tabs)) : saved.delete(project)),
      };
    }

    it("saves each project's tabs with their shells and the active one", async () => {
      const core = new FakeTerminals();
      const store = memory();
      const sessions = new TerminalSessions(core, store);
      const first = sessions.add("p1");
      await sessions.start(first, SIZE);
      const second = sessions.add("p1", "src");
      expect(store.saved.get("p1")).toEqual({
        tabs: [
          { id: "t1", title: "Terminal 1" },
          { id: null, title: "Terminal 2", cwd: "src" },
        ],
        active: 1,
      });
      sessions.close(second);
      sessions.close(first);
      expect(store.saved.has("p1")).toBe(false);
    });

    it("takes the shell back with its scrollback, size and held output", async () => {
      const core = new FakeTerminals();
      const store = memory();
      const before = new TerminalSessions(core, store);
      const old = before.add("p1");
      await before.start(old, SIZE);
      core.output("t1", "one\r\n");

      const sessions = new TerminalSessions(core, store);
      sessions.listen();
      expect(sessions.restore("p1")).toBe(1);
      expect(sessions.restore("p1")).toBe(0);
      const [tab] = sessions.of("p1");
      if (!tab) throw new Error("no tab");
      const screen = view();
      tab.attach(screen);
      sessions.input(tab, "typed");
      let release = () => {};
      core.scrollbackGate = new Promise((resolve) => (release = resolve));
      const started = sessions.start(tab, { cols: 120, rows: 40 });
      core.output("t1", "two\r\n");
      release();
      await started;
      await settle();
      expect(tab.state).toEqual({ kind: "running", id: "t1" });
      expect(screen.screen).toBe("one\r\ntwo\r\ntyped");
      expect(core.open.get("t1")?.size).toEqual({ cols: 120, rows: 40 });
      expect(core.methods().filter((m) => m === "terminals.open")).toHaveLength(1);
    });

    it("shows a shell that ended before the reload as ended", async () => {
      const core = new FakeTerminals();
      const store = memory();
      const before = new TerminalSessions(core, store);
      await before.start(before.add("p1"), SIZE);
      core.output("t1", "done\r\n");
      core.exit("t1", 0);

      const sessions = new TerminalSessions(core, store);
      sessions.restore("p1");
      const [tab] = sessions.of("p1");
      if (!tab) throw new Error("no tab");
      const screen = view();
      tab.attach(screen);
      await sessions.start(tab, SIZE);
      expect(tab.state).toEqual({ kind: "exited", code: undefined });
      expect(screen.screen).toBe("done\r\n\r\nProcess ended.\r\n");
    });

    it("waits for the connection instead of starting a second shell", async () => {
      const core = new FakeTerminals();
      const store = memory();
      const before = new TerminalSessions(core, store);
      await before.start(before.add("p1"), SIZE);
      core.output("t1", "one\r\n");

      core.down = true;
      const sessions = new TerminalSessions(core, store);
      sessions.listen();
      sessions.restore("p1");
      const [tab] = sessions.of("p1");
      if (!tab) throw new Error("no tab");
      const screen = view();
      tab.attach(screen);
      await sessions.start(tab, SIZE);
      sessions.resize(tab, { cols: 100, rows: 30 });
      expect(tab.state.kind).toBe("starting");
      expect(store.saved.get("p1")?.tabs).toEqual([{ id: "t1", title: "Terminal 1" }]);

      core.down = false;
      core.emit({ type: "info.resync", data: { skipped: 0 } });
      await settle();
      expect(tab.state).toEqual({ kind: "running", id: "t1" });
      expect(screen.screen).toBe("one\r\n");
      expect(core.open.get("t1")?.size).toEqual({ cols: 100, rows: 30 });
      expect(core.methods().filter((m) => m === "terminals.open")).toHaveLength(1);
    });

    it("closes a saved shell whose tab is closed before it was taken back", async () => {
      const store = memory();
      store.save("p1", { tabs: [{ id: "t1", title: "Terminal 1" }], active: 0 });
      const core = new FakeTerminals();
      const sessions = new TerminalSessions(core, store);
      sessions.restore("p1");
      const [tab] = sessions.of("p1");
      if (!tab) throw new Error("no tab");
      sessions.close(tab);
      expect(core.calls).toEqual([{ method: "terminals.close", params: { id: "t1" } }]);
    });

    it("starts a new shell in the tab's folder when the core no longer knows the old one", async () => {
      const store = memory();
      store.save("p1", { tabs: [{ id: "t9", title: "Terminal 4", cwd: "src" }], active: 0 });
      const core = new FakeTerminals();
      const sessions = new TerminalSessions(core, store);
      sessions.restore("p1");
      const [tab] = sessions.of("p1");
      if (!tab) throw new Error("no tab");
      await sessions.start(tab, SIZE);
      expect(tab.state).toEqual({ kind: "running", id: "t1" });
      expect(core.calls.at(-1)).toEqual({
        method: "terminals.open",
        params: { project: "p1", size: SIZE, cwd: "src" },
      });
      expect(store.saved.get("p1")?.tabs).toEqual([{ id: "t1", title: "Terminal 4", cwd: "src" }]);
      expect(sessions.add("p1").title).toBe("Terminal 5");
    });
  });
});
