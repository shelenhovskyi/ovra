import { flushSync } from "svelte";
import { expect, test, vi } from "vitest";
import type { Project, ProjectState, Request } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { OpenFiles, persistOpenFiles } from "./open-files.svelte";

function fakeCore(initial: ProjectState) {
  let state = initial;
  const calls: string[] = [];
  const sent: unknown[] = [];
  const transport = {
    onEvent: () => () => {},
    call: async (request: Request) => {
      calls.push(request.method);
      if (request.method === "projects.get_state") return { type: "projects.state", data: state };
      if (request.method === "projects.set_state") {
        sent.push(request.params.state);
        state = { ...state, ...request.params.state };
        return { type: "ok" };
      }
      throw new Error(`unexpected ${request.method}`);
    },
  } as unknown as Transport;
  return { transport, calls, sent, state: () => state };
}

const empty: ProjectState = { open_files: [], active_file: null, expanded_dirs: [] };
const project: Project = { id: "p1", name: "p", root: "/p", last_opened_ms: null };

test("writes only its own fields and moves a reopened file to the end", async () => {
  const core = fakeCore({ open_files: ["a.rs", "b.rs"], active_file: "b.rs", expanded_dirs: ["src"] });
  const files = new OpenFiles(core.transport, "p1");
  expect(await files.restore()).toBe("b.rs");

  await files.opened("a.rs");
  expect(core.sent.at(-1)).toEqual({ open_files: ["b.rs", "a.rs"], active_file: "a.rs" });
  // Another panel changes its field meanwhile; it must survive our writes.
  await core.transport.call({
    method: "projects.set_state",
    params: { id: "p1", state: { expanded_dirs: ["src", "docs"] } },
  });
  await files.opened("b.rs");
  await files.opened("a.rs");
  expect(core.state()).toEqual({ open_files: ["b.rs", "a.rs"], active_file: "a.rs", expanded_dirs: ["src", "docs"] });

  await files.opened(null);
  expect(core.state()).toEqual({ open_files: ["b.rs", "a.rs"], active_file: null, expanded_dirs: ["src", "docs"] });
});

test("keeps at most 20 open files", async () => {
  const core = fakeCore(empty);
  const files = new OpenFiles(core.transport, "p1");
  await files.restore();
  for (let i = 0; i < 25; i++) void files.opened(`f${i}`);
  await files.opened("f24");
  expect(core.state().open_files).toHaveLength(20);
  expect(core.state().open_files[0]).toBe("f5");
});

test("reopens the active file when a project opens, then records changes", async () => {
  const core = fakeCore({ open_files: ["main.rs"], active_file: "main.rs", expanded_dirs: [] });
  const workspace = new Workspace();
  const stop = persistOpenFiles(core.transport, workspace);
  workspace.project = project;
  flushSync();
  await vi.waitFor(() => expect(workspace.openPath).toBe("main.rs"));
  expect(core.calls).toEqual(["projects.get_state"]);

  workspace.openFile("lib.rs");
  flushSync();
  await vi.waitFor(() => expect(core.state().active_file).toBe("lib.rs"));
  expect(core.state().open_files).toEqual(["main.rs", "lib.rs"]);
  stop();
});
