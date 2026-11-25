import { describe, expect, it } from "vitest";
import { FakeLspCore, server } from "./fake-lsp";
import { LanguageServerStatus, languageServers } from "./servers.svelte";

const settle = async () => {
  for (let i = 0; i < 4; i++) await Promise.resolve();
};

async function opened(core = new FakeLspCore()) {
  const state = new LanguageServerStatus(core);
  await state.show("p1");
  return { core, state };
}

const ids = (state: LanguageServerStatus) => state.servers.map((s) => `${s.id}:${s.state}`);

describe("LanguageServerStatus", () => {
  it("loads the project's status and leaves stopped servers out", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a"), server("b", { state: "stopped" })];
    core.known = [{ language: "go", name: "gopls", path: null, install: "go install golang.org/x/tools/gopls@latest" }];
    const { state } = await opened(core);
    expect(core.calls).toEqual([{ method: "lsp.status", params: { project: "p1" } }]);
    expect(ids(state)).toEqual(["a:running"]);
    expect(state.status?.trusted).toBe(true);
    expect(state.missing.map((k) => k.name)).toEqual(["gopls"]);
  });

  it("follows lsp.changed for its own project only", async () => {
    const { core, state } = await opened();
    core.changed(server("a", { state: "starting" }));
    core.changed(server("x", { project: "p2" }));
    core.changed(server("a"));
    core.changed(server("t", { language: "typescript", name: "typescript-language-server" }));
    expect(ids(state)).toEqual(["a:running", "t:running"]);
    core.changed(server("a", { state: "stopped" }));
    expect(ids(state)).toEqual(["t:running"]);
  });

  it("keeps a failed server with its reason until a new start for its language", async () => {
    const { core, state } = await opened();
    core.changed(server("a", { state: "failed", reason: "exited with code 1" }));
    core.changed(server("t", { language: "typescript", state: "starting" }));
    expect(state.servers[0]).toMatchObject({ id: "a", state: "failed", reason: "exited with code 1" });
    core.changed(server("b", { state: "starting" }));
    expect(ids(state)).toEqual(["t:starting", "b:starting"]);
  });

  it("reloads on info.resync and drops failed servers the core no longer has", async () => {
    const { core, state } = await opened();
    core.changed(server("a", { state: "failed", reason: "crashed" }));
    core.servers = [server("b")];
    core.resync();
    await settle();
    expect(core.calls.filter((c) => c.method === "lsp.status")).toHaveLength(2);
    expect(ids(state)).toEqual(["b:running"]);
  });

  it("clears what it shows for another project", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a")];
    const { state } = await opened(core);
    const next = state.show("p2");
    expect(state.servers).toEqual([]);
    await next;
    expect(core.calls.at(-1)).toEqual({ method: "lsp.status", params: { project: "p2" } });
  });

  it("stops a server through lsp.stop", async () => {
    const { core, state } = await opened();
    await state.stop("a");
    expect(core.calls.at(-1)).toEqual({ method: "lsp.stop", params: { server: "a" } });
  });

  it("hints only at missing servers the editor needed", async () => {
    const core = new FakeLspCore();
    core.known = [
      {
        language: "rust",
        name: "rust-analyzer",
        path: "/bin/rust-analyzer",
        install: "rustup component add rust-analyzer",
      },
      { language: "go", name: "gopls", path: null, install: "go install golang.org/x/tools/gopls@latest" },
      { language: "python", name: "pyright", path: null, install: "npm install -g pyright" },
    ];
    const { state } = await opened(core);
    expect(state.hints).toEqual([]);
    state.needServer("go");
    state.needServer("rust");
    expect(state.hints.map((k) => k.name)).toEqual(["gopls"]);
  });

  it("is shared per transport", () => {
    const core = new FakeLspCore();
    expect(languageServers(core)).toBe(languageServers(core));
  });
});
