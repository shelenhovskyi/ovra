import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import Teach from "./Teach.svelte";
import { rememberRule } from "./teach";

let cleanup: (() => void)[] = [];

afterEach(() => {
  for (const off of cleanup) off();
  cleanup = [];
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

/** A project of `files`; writes change them and are kept in `writes`. */
function fake(files: Record<string, string>) {
  const writes: { path: string; text: string; expected: number | null }[] = [];
  const transport: Transport = {
    call: ((request: Request) => {
      if (request.method === "fs.read_file") {
        const text = files[request.params.path];
        if (text === undefined) return Promise.reject(new CoreError({ code: "not_found", message: "no such file" }));
        return Promise.resolve({
          type: "fs.file",
          data: { path: request.params.path, size: text.length, mtime_ms: 7, body: { kind: "text", text } },
        });
      }
      if (request.method === "fs.write_file") {
        const { path, text, expected_mtime_ms } = request.params;
        writes.push({ path, text, expected: expected_mtime_ms });
        files[path] = text;
        return Promise.resolve({ type: "fs.written", data: { mtime_ms: 8 } });
      }
      return Promise.reject(new Error(`unexpected ${request.method}`));
    }) as Transport["call"],
    onEvent: () => () => {},
  };
  return { transport, writes };
}

function show(transport: Transport) {
  const result = { done: [] as string[], cancelled: 0 };
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(Teach, {
    target,
    props: { transport, project: "p1", done: (path) => result.done.push(path), cancel: () => result.cancelled++ },
  });
  cleanup.push(() => {
    void unmount(app);
    target.remove();
  });
  return { target, result };
}

function type(target: HTMLElement, id: string, value: string): void {
  const field = target.querySelector<HTMLTextAreaElement>(`[data-testid=${id}]`);
  if (!field) throw new Error(`no ${id}`);
  field.value = value;
  field.dispatchEvent(new Event("input"));
}

it("suggests the found commands and writes a new AGENTS.md and the CLAUDE.md import", async () => {
  const { transport, writes } = fake({ justfile: "# unit tests\ntest:\n  cargo test\n", "CLAUDE.md": "# Claude\n" });
  const { target, result } = show(transport);
  await settle();
  expect(target.querySelector<HTMLTextAreaElement>("[data-testid=teach-commands]")?.value).toBe(
    "`just test` - unit tests",
  );
  type(target, "teach-never", "migrations/");
  flushSync();
  expect(target.querySelector("[data-testid=teach-preview]")?.textContent).toContain(
    "## Do not touch\n\n- migrations/",
  );
  target.querySelector<HTMLButtonElement>("[data-testid=teach-save]")?.click();
  await settle();
  expect(writes.map((w) => [w.path, w.expected])).toEqual([
    ["AGENTS.md", null],
    ["CLAUDE.md", 7],
  ]);
  expect(writes[0]?.text).toContain("- `just test` - unit tests");
  expect(writes[1]?.text).toBe("@AGENTS.md\n\n# Claude\n");
  expect(result.done).toEqual(["AGENTS.md"]);
});

it("adds to an existing AGENTS.md, and saves nothing when nothing changes", async () => {
  const { transport, writes } = fake({ "AGENTS.md": "# Ours\n\n## Rules\n\n- Be brief\n" });
  const { target, result } = show(transport);
  await settle();
  expect(target.querySelector("[data-testid=teach-link]")).toBeNull();
  const save = target.querySelector<HTMLButtonElement>("[data-testid=teach-save]");
  expect(save?.getAttribute("aria-disabled")).toBe("true");
  save?.click();
  await settle();
  expect(writes).toEqual([]);
  type(target, "teach-rules", "Be brief\nWrite tests");
  flushSync();
  save?.click();
  await settle();
  expect(writes).toEqual([
    { path: "AGENTS.md", text: "# Ours\n\n## Rules\n\n- Be brief\n- Write tests\n", expected: 7 },
  ]);
  expect(result.done).toEqual(["AGENTS.md"]);
});

it("remembers a review comment as one rule", async () => {
  const { transport, writes } = fake({});
  await rememberRule(transport, "p1", "Use the fake clock\nin tests");
  expect(writes).toHaveLength(1);
  expect(writes[0]?.expected).toBeNull();
  expect(writes[0]?.text).toMatch(/## Rules\n\n- Use the fake clock in tests\n$/);
  await rememberRule(transport, "p1", "Use the fake clock in tests");
  expect(writes).toHaveLength(1);
});
