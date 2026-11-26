import { afterEach, expect, test, vi } from "vitest";
import { listCommands } from "../keys/commands";
import type { Project } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { registerFileCommands } from "./file-commands";
import { tabs } from "./open-files.svelte";

afterEach(() => {
  tabs.notice = null;
  vi.unstubAllGlobals();
});

const project: Project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
const run = (id: string) =>
  listCommands()
    .find((c) => c.id === id)
    ?.run();

test("copy the open file's path and open it in another app, saying why that failed", async () => {
  const writeText = vi.fn(async () => {});
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const call = vi.fn(async () => {
    throw new CoreError({ code: "forbidden", message: "it could run as a program" });
  });
  const unregister = registerFileCommands({ project, openPath: "src/a.sh" }, { call } as unknown as Transport);

  await run("file.copy_path");
  expect(writeText).toHaveBeenCalledWith("/demo/src/a.sh");
  await run("file.copy_relative_path");
  expect(writeText).toHaveBeenLastCalledWith("src/a.sh");
  await run("file.open_external");
  expect(call).toHaveBeenCalledWith({
    method: "open.external",
    params: { project: "p1", path: "src/a.sh", line: null },
  });
  expect(tabs.notice).toBe("Could not open src/a.sh in another app: it could run as a program.");
  unregister();
});
