import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { registerCommand } from "../keys/commands";
import { Workspace } from "../shell/workspace.svelte";
import { FakeLspCore, server } from "./fake-lsp";
import { languageServers } from "./servers.svelte";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
});

async function settle() {
  for (let i = 0; i < 8; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

async function mountWith(core: FakeLspCore) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: core.trusted };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return workspace;
}

const $ = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector);
const item = () => $<HTMLButtonElement>("[data-testid=lsp-status]");

async function openPanel() {
  item()?.click();
  await settle();
}

describe("LanguageServers", () => {
  it("stays hidden while the project has no servers", async () => {
    await mountWith(new FakeLspCore());
    expect(item()).toBeNull();
  });

  it("names one server and counts several", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a")];
    await mountWith(core);
    expect(item()?.textContent?.trim()).toBe("rust-analyzer");
    expect(item()?.getAttribute("aria-label")).toBe("rust-analyzer");
    core.changed(server("t", { language: "typescript", name: "typescript-language-server" }));
    flushSync();
    expect(item()?.textContent?.trim()).toBe("2 language servers");
  });

  it("opens with aria-expanded, closes on Escape and returns focus", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a")];
    await mountWith(core);
    item()?.focus();
    await openPanel();
    expect(item()?.getAttribute("aria-expanded")).toBe("true");
    expect(document.activeElement?.id).toBe("lsp-title");
    expect($("[data-testid=lsp-server]")?.textContent).toContain(
      "In the sandbox: reads the project, writes only its own cache, no network (bubblewrap",
    );
    $("[data-testid=lsp-panel]")?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
    flushSync();
    expect($("[data-testid=lsp-panel]")).toBeNull();
    expect(document.activeElement).toBe(item());
  });

  it("names the server of the shown file, or the missing one it needs", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a"), server("t", { language: "typescript", name: "tsserver" })];
    core.known = [{ language: "python", name: "pyright", path: null, install: "pip install pyright" }];
    const workspace = await mountWith(core);
    workspace.openPath = "src/app.ts";
    flushSync();
    expect(item()?.textContent?.trim()).toBe("tsserver +1");
    languageServers(core).needServer("python");
    workspace.openPath = "main.py";
    flushSync();
    expect(item()?.textContent?.trim()).toBe("!pyright not installed");
    workspace.openPath = "main.rs";
    flushSync();
    expect(item()?.textContent?.trim()).toBe("rust-analyzer +1");
  });

  it("warns only about a server without a sandbox", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a"), server("t", { language: "typescript", name: "tsserver", sandbox: null })];
    await mountWith(core);
    expect(item()?.getAttribute("aria-label")).toBe("2 language servers, needs attention");
    await openPanel();
    const warnings = [...target.querySelectorAll("[data-testid=lsp-unsandboxed]")];
    expect(warnings).toHaveLength(1);
    expect(warnings[0]?.closest("li")?.textContent).toContain("tsserver");
    expect(warnings[0]?.textContent).toContain(
      "Runs without a sandbox: it can read and change your files and reach the network.",
    );
  });

  it("shows why a server failed, without a Stop button", async () => {
    const core = new FakeLspCore();
    await mountWith(core);
    core.changed(server("a", { state: "failed", reason: "rust-analyzer exited with code 101" }));
    flushSync();
    expect(item()?.getAttribute("aria-label")).toBe("rust-analyzer, needs attention");
    await openPanel();
    expect($("[data-testid=lsp-reason]")?.textContent).toBe("rust-analyzer exited with code 101");
    expect($("[data-testid=lsp-server] button")).toBeNull();
  });

  it("stops a server with lsp.stop and drops it once it stopped", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a"), server("t", { language: "typescript", name: "tsserver" })];
    await mountWith(core);
    await openPanel();
    $<HTMLButtonElement>("[aria-label='Stop tsserver']")?.click();
    await settle();
    expect(core.calls.at(-1)).toEqual({ method: "lsp.stop", params: { server: "t" } });
    core.changed(server("t", { language: "typescript", name: "tsserver", state: "stopped" }));
    flushSync();
    expect([...target.querySelectorAll("[data-testid=lsp-server] strong")].map((el) => el.textContent)).toEqual([
      "rust-analyzer",
    ]);
    expect(document.activeElement?.id).toBe("lsp-title");
    expect($("[role=status]")?.textContent).toBe(
      "Stopped tsserver. It starts again the next time you open a typescript file.",
    );
  });

  it("closes when a click lands elsewhere, without taking focus", async () => {
    const core = new FakeLspCore();
    core.servers = [server("a")];
    await mountWith(core);
    await openPanel();
    const elsewhere = document.body.appendChild(document.createElement("button"));
    elsewhere.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    flushSync();
    expect($("[data-testid=lsp-panel]")).toBeNull();
    elsewhere.remove();
  });

  it("links an untrusted project's panel to its trust card", async () => {
    const core = new FakeLspCore();
    core.trusted = false;
    let asked = 0;
    const off = registerCommand({ id: "projects.review_trust", title: "t", run: () => void asked++ });
    await mountWith(core);
    languageServers(core).needServer("rust");
    flushSync();
    // "Turn on" beside the item goes straight there.
    $<HTMLButtonElement>("[data-testid=lsp-turn-on]")?.click();
    await settle();
    expect(asked).toBe(1);
    await openPanel();
    [...target.querySelectorAll("button")].find((b) => b.textContent === "Review and trust…")?.click();
    await settle();
    expect(asked).toBe(2);
    expect($("[data-testid=lsp-panel]")).toBeNull();
    off();
  });

  it("says why servers do not start in an untrusted project", async () => {
    const core = new FakeLspCore();
    core.trusted = false;
    core.known = [{ language: "go", name: "gopls", path: null, install: "go install golang.org/x/tools/gopls@latest" }];
    await mountWith(core);
    expect(item()).toBeNull();
    languageServers(core).needServer("rust");
    flushSync();
    expect(item()?.textContent?.trim()).toBe("Code intelligence: off");
    await openPanel();
    expect($("[data-testid=lsp-untrusted]")?.textContent?.trim()).toBe(
      "Language servers run this project's code, so they start only in trusted projects.",
    );
    expect($("[data-testid=lsp-missing] code")?.textContent).toBe("go install golang.org/x/tools/gopls@latest");
  });

  it("shows a missing server once the editor needed it", async () => {
    const core = new FakeLspCore();
    core.known = [{ language: "go", name: "gopls", path: null, install: "go install golang.org/x/tools/gopls@latest" }];
    await mountWith(core);
    expect(item()).toBeNull();
    languageServers(core).needServer("go");
    flushSync();
    expect(item()?.textContent?.trim()).toBe("!gopls not installed");
    expect(item()?.getAttribute("aria-label")).toBe("gopls not installed, needs attention");
    await openPanel();
    expect($("[data-testid=lsp-untrusted]")).toBeNull();
    expect($("[data-testid=lsp-missing]")?.textContent).toContain("gopls");
  });
});
