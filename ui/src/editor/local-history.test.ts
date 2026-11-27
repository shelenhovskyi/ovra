import { afterEach, expect, test } from "vitest";
import { palette } from "../palette/mode.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { Drafts } from "./drafts.svelte";
import { FakeCore, MemoryStorage } from "./fake-core";
import { showLocalHistory } from "./history-list";
import { LocalHistory, MemoryVersions, mayHoldSecrets, VERSIONS_PER_FILE, versionLabel } from "./local-history";
import { OpenFiles, tabs } from "./open-files.svelte";

afterEach(() => {
  tabs.current = null;
  tabs.notice = null;
  palette.list = null;
});

function setup() {
  const store = new MemoryVersions();
  let clock = Date.now();
  const history = new LocalHistory(store, () => clock++);
  const core = new FakeCore();
  const buffers = new Buffers({
    transport: core,
    project: "p1",
    drafts: new Drafts(new MemoryStorage()),
    highlighter: () => null,
    history,
  });
  return { store, history, core, buffers };
}

test("a version is kept on open, save and outside change, never twice in a row", async () => {
  const { store, history, core, buffers } = setup();
  core.put("a.txt", "one\r\ntwo");
  const a = buffers.open("a.txt");
  await a.loaded;
  a.edit({ changes: { from: 0, insert: "zero " } });
  await a.save();
  // Saving the same text again keeps nothing new.
  await a.save();
  core.put("a.txt", "from an agent");
  await a.diskChanged();
  const versions = await history.versions("p1", "a.txt");
  expect(versions.map((v) => [v.reason, v.text])).toEqual([
    ["changed_on_disk", "from an agent"],
    ["saved", "zero one\ntwo"],
    ["opened", "one\ntwo"],
  ]);
  // Opening again in a new window, with the same text, adds nothing.
  const again = setup();
  again.store.versions.push(...store.versions);
  again.core.put("a.txt", "from an agent");
  const b = again.buffers.open("a.txt");
  await b.loaded;
  expect(await again.history.versions("p1", "a.txt")).toHaveLength(3);
});

test("only the newest versions stay; secret-looking and large files are never kept", async () => {
  const { history } = setup();
  for (let i = 0; i < VERSIONS_PER_FILE + 5; i++) void history.record("p1", "a.txt", `v${i}`, "saved");
  const versions = await history.versions("p1", "a.txt");
  expect(versions).toHaveLength(VERSIONS_PER_FILE);
  expect(versions[0]?.text).toBe(`v${VERSIONS_PER_FILE + 4}`);
  await history.record("p1", "config/.env.local", "TOKEN=x", "saved");
  await history.record("p1", "big.txt", "x".repeat(1024 * 1024 + 1), "saved");
  expect(await history.versions("p1", "config/.env.local")).toEqual([]);
  expect(await history.versions("p1", "big.txt")).toEqual([]);
  expect(["id.pem", "server.KEY", "credentials.json", "env.ts"].map(mayHoldSecrets)).toEqual([true, true, true, false]);
});

test("a failing store leaves the file working, without history", async () => {
  const broken = new MemoryVersions();
  broken.add = () => Promise.reject(new Error("quota"));
  const history = new LocalHistory(broken);
  await expect(history.record("p1", "a.txt", "x", "saved")).resolves.toBeUndefined();
});

test("labels say what happened and when, with the date for other days", () => {
  const today = new Date(2026, 10, 27, 14, 32).getTime();
  expect(versionLabel({ project: "p1", path: "a", time: today, text: "", reason: "saved" }, today)).toBe(
    "Saved today at 14:32",
  );
  const before = new Date(2026, 10, 20, 9, 5).getTime();
  expect(versionLabel({ project: "p1", path: "a", time: before, text: "", reason: "changed_on_disk" }, today)).toBe(
    "Changed on disk 20 Nov 2026 at 09:05",
  );
});

test("picking a version puts its text in the editor as an unsaved edit", async () => {
  const { history, core, buffers } = setup();
  core.put("a.txt", "first");
  tabs.current = new OpenFiles(core, "p1", buffers);
  const workspace = new Workspace();
  workspace.openPath = "a.txt";
  // No buffer yet: nothing is kept, the notice says when versions come.
  await showLocalHistory(workspace, async () => {}, history);
  expect(tabs.notice).toContain("No earlier versions of a.txt yet");
  const a = buffers.open("a.txt");
  await a.loaded;
  a.edit({ changes: { from: 0, to: 5, insert: "second" } });
  await a.save();
  let edited = false;
  await showLocalHistory(
    workspace,
    async () => {
      edited = true;
    },
    history,
  );
  expect(workspace.paletteOpen).toBe(true);
  const entries = await palette.list?.entries();
  expect(entries?.map((e) => e.detail)).toEqual(["same as now", "1 line"]);
  await entries?.[1]?.run();
  expect(edited).toBe(true);
  expect([a.text, a.dirty]).toEqual(["first", true]);
  expect(core.files.get("a.txt")?.body).toEqual({ kind: "text", text: "second" });
  expect(tabs.notice).toMatch(/^Put back the text opened today at \d\d:\d\d\. Save keeps it/);
});
