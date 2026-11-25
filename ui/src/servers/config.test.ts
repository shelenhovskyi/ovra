import { describe, expect, it } from "vitest";
import { addServer, CONFIG, checkForm } from "./config";
import { FakeServersCore } from "./fake-servers";

const form = { name: "web", command: "pnpm dev", folder: "", port: "5173" };
const web = { name: "web", command: ["pnpm", "dev"], port: 5173 };

describe("the servers file", () => {
  it("checks the form as the core checks the file", () => {
    expect(checkForm(form, [])).toEqual({ def: web });
    expect(checkForm({ ...form, folder: "./ui" }, [])).toEqual({ def: { ...web, cwd: "./ui" } });
    const field = (over: Partial<typeof form>, defined = [] as (typeof web)[]) => {
      const checked = checkForm({ ...form, ...over }, defined);
      return "field" in checked ? checked.field : null;
    };
    expect(field({ name: "-web" })).toBe("name");
    expect(field({}, [web])).toBe("name");
    expect(field({ command: "  " })).toBe("command");
    for (const folder of ["/abs", "../up", "c:/x", "a//b"]) expect(field({ folder })).toBe("folder");
    for (const port of ["80", "3128", "70000", "5173.5", "web"]) expect(field({ port })).toBe("port");
    expect(field({ name: "api" }, [web])).toBe("port");
  });

  it("appends to a file that exists, refusing a change made since it was read", async () => {
    const core = new FakeServersCore();
    core.files.set(CONFIG, { text: JSON.stringify({ servers: [web] }), mtime_ms: 7 });
    const api = { name: "api", command: ["cargo", "run"], port: 8080 };
    await addServer(core, "p1", api);
    expect(JSON.parse(core.files.get(CONFIG)?.text ?? "")).toEqual({ servers: [web, api] });
    expect(core.calls.at(-1)).toMatchObject({ method: "fs.write_file", params: { expected_mtime_ms: 7 } });

    core.files.set(CONFIG, { text: "{ not json", mtime_ms: 9 });
    await expect(addServer(core, "p1", api)).rejects.toThrow(/Open it to fix it/);
  });
});
