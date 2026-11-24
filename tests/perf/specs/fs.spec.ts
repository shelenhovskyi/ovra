// fs-index through the core: git status, the watcher's walk, listings and
// change events on the 100k-file repository.
import { appendFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "@playwright/test";
import { Client, type Daemon, type Event, median, memoryMb, REPO, startDaemon, timed } from "../daemon";
import { report } from "../metrics";
import { sourceFile } from "../repo";

type Listing = { entries: unknown[] };

let daemon: Daemon;
let client: Client;
let project: string;

test.beforeAll(async () => {
  daemon = await startDaemon();
  client = await Client.connect(daemon.url);
  project = (await client.call<{ id: string }>("projects.add", { root: REPO })).id;
});

test.afterAll(async () => {
  client?.close();
  await daemon?.stop();
});

const list = (path: string) => client.call<Listing>("fs.list_dir", { project, path });
const changed = (path: string) => (e: Event) => e.type === "fs.changed" && (e.data?.paths ?? []).includes(path);

/** Makes a change on disk and waits for the event it should cause. */
async function change(make: () => void, event: Promise<Event>): Promise<void> {
  make();
  await event;
}

async function medianOf(runs: number, work: () => Promise<unknown>): Promise<number> {
  const times: number[] = [];
  for (let i = 0; i < runs; i++) times.push(await timed(work));
  return median(times);
}

test("git status and the initial walk", async () => {
  // Not watched yet, so every listing runs `git status` over the whole repository.
  await list("");
  report("fs.git_status_ms", await medianOf(3, () => list("")));
  // Opening starts the watcher, which walks every directory that is not ignored.
  // Opening the project that is already active is a no-op, so switch to a
  // tiny project in between: every timed open then starts a fresh watcher.
  const tinyRoot = mkdtempSync(join(tmpdir(), "ovra-perf-tiny-"));
  writeFileSync(join(tinyRoot, "a.txt"), "a\n");
  const tiny = (await client.call<{ id: string }>("projects.add", { root: tinyRoot })).id;
  const opens: number[] = [];
  for (let i = 0; i < 3; i++) {
    await client.call("projects.open", { id: tiny });
    opens.push(await timed(() => client.call("projects.open", { id: project })));
  }
  rmSync(tinyRoot, { recursive: true, force: true });
  report("fs.watch_start_ms", median(opens));
});

test("listings of a watched project", async () => {
  await client.call("projects.open", { id: project });
  await list(""); // fills the cached git status
  report("fs.list_root_ms", await medianOf(10, () => list("")));
  report("fs.list_leaf_ms", await medianOf(10, () => list("pkg05/mod05/part05")));
  report("fs.list_flat_ms", await medianOf(10, () => list("flat")));
  const flat = await list("flat");
  test.expect(flat.entries.length).toBeGreaterThan(4_999);
});

test("watcher latency", async () => {
  await client.call("projects.open", { id: project });
  await list("");
  const dir = "pkg03/mod04/part05";

  const created: number[] = [];
  for (let i = 0; i < 5; i++) {
    const path = `${dir}/perf-new-${i}.rs`;
    const event = client.nextEvent(changed(path));
    created.push(await timed(() => change(() => writeFileSync(join(REPO, path), "// new\n"), event)));
  }
  report("fs.watch_create_ms", median(created));

  const fresh = "docs/perf-fresh";
  try {
    const parent = client.nextEvent(changed(fresh));
    mkdirSync(join(REPO, fresh));
    await parent;
    const inDir: number[] = [];
    for (let i = 0; i < 3; i++) {
      const dirPath = `${fresh}/round${i}`;
      const first = client.nextEvent(changed(dirPath));
      mkdirSync(join(REPO, dirPath));
      await first;
      // The new directory is watched as soon as its creation is reported.
      const path = `${dirPath}/note.md`;
      const event = client.nextEvent(changed(path));
      inDir.push(await timed(() => change(() => writeFileSync(join(REPO, path), "note\n"), event)));
    }
    report("fs.watch_new_dir_ms", median(inDir));

    const tracked = sourceFile(7, 7, 7, 7);
    const original = readFileSync(join(REPO, tracked), "utf8");
    const gitTimes: number[] = [];
    try {
      for (let i = 0; i < 3; i++) {
        const isGit = (e: Event) => e.type === "fs.git_changed";
        let event = client.nextEvent(isGit);
        gitTimes.push(await timed(() => change(() => appendFileSync(join(REPO, tracked), `// edit ${i}\n`), event)));
        event = client.nextEvent(isGit);
        writeFileSync(join(REPO, tracked), original);
        await event;
      }
    } finally {
      writeFileSync(join(REPO, tracked), original);
    }
    report("fs.watch_git_ms", median(gitTimes));
  } finally {
    rmSync(join(REPO, fresh), { recursive: true, force: true });
    for (let i = 0; i < 5; i++) rmSync(join(REPO, `${dir}/perf-new-${i}.rs`), { force: true });
  }

  const mem = memoryMb(daemon.pid);
  report("memory.daemon_peak_mb", mem?.peak ?? null);
});
