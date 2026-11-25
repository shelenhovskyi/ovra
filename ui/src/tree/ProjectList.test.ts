import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { ErrorCode, Project } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { FakeCore } from "./fake-core";
import { FolderOpener } from "./folder-opener.svelte";
import ProjectList from "./ProjectList.svelte";

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = "";
});

async function settle(): Promise<void> {
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

const projects: Project[] = [
  { id: "p1", name: "app", root: "/work/one/app", last_opened_ms: null, trusted: false },
  { id: "p2", name: "app", root: "/work/two/app", last_opened_ms: null, trusted: false },
  { id: "p3", name: "docs", root: "/work/docs", last_opened_ms: null, trusted: false },
];

/** Lists nothing and answers `projects.add` with the given error, or holds it until `release`. */
class FailingAdd implements Transport {
  release: () => void = () => {};
  constructor(private readonly error: { code: ErrorCode; message: string } | null) {}
  async call(request: { method: string }) {
    if (request.method === "projects.list") return { type: "projects.list", data: [] } as never;
    if (this.error) throw new CoreError(this.error);
    await new Promise<void>((resolve) => {
      this.release = resolve;
    });
    throw new CoreError({ code: "internal", message: "store crashed" });
  }
  onEvent() {
    return () => {};
  }
}

function mountList(transport: Transport) {
  const opened: string[] = [];
  const target = document.body.appendChild(document.createElement("div"));
  const opener = new FolderOpener(transport, (p) => opened.push(p.id));
  const app = mount(ProjectList, { target, props: { transport, opener } });
  cleanup = () => unmount(app);
  const input = () => target.querySelector("input") as HTMLInputElement;
  const submit = () => target.querySelector<HTMLButtonElement>("button[type=submit]") as HTMLButtonElement;
  const error = () => target.querySelector("[role=alert]")?.textContent ?? null;
  const type = (value: string) => {
    input().value = value;
    input().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  };
  const send = async (value: string) => {
    type(value);
    target.querySelector("form")?.requestSubmit();
    await settle();
  };
  return { target, opened, input, submit, error, type, send };
}

describe("ProjectList start screen", () => {
  it("asks for a full path before calling the core", async () => {
    const t = mountList(new FailingAdd({ code: "internal", message: "should not be called" }));
    await settle();
    for (const path of ["~/project", "small"]) {
      t.type(path);
      t.target.querySelector("form")?.requestSubmit();
      flushSync();
      expect(t.error()).toBe("Use the full path to a folder, for example /home/you/my-app.");
    }
  });

  it.each([
    ["not_found", "/no/such: No such file", "Folder not found: /no/such"],
    [
      "bad_request",
      "/x/README.md is not a folder",
      "That is not a folder. Choose the folder itself, not a file in it.",
    ],
    ["bad_request", "/root: Permission denied", "/root: Permission denied"],
    ["unavailable", "db locked", "The project store is not available right now."],
    ["internal", "disk on fire", "disk on fire"],
  ] as const)("turns %s (%s) into a human message", async (code, message, expected) => {
    const t = mountList(new FailingAdd({ code, message }));
    await settle();
    await t.send(code === "not_found" ? "/no/such" : "/x/README.md");
    expect(t.error()).toBe(expected);
  });

  it("ties the error to the field and clears it on edit", async () => {
    const t = mountList(new FailingAdd({ code: "not_found", message: "gone" }));
    await settle();
    expect(t.input().labels?.[0]?.textContent).toBe("Open a folder");
    expect(t.input().getAttribute("aria-invalid")).toBeNull();
    await t.send("/gone");
    expect(t.input().getAttribute("aria-invalid")).toBe("true");
    const described = t.input().getAttribute("aria-describedby")?.split(" ") ?? [];
    // The error takes the hint's place: it says the same and more.
    expect(described.map((id) => document.getElementById(id)?.textContent)).toEqual(["Folder not found: /gone"]);
    t.type("/gone2");
    expect(t.error()).toBeNull();
    expect(t.input().getAttribute("aria-invalid")).toBeNull();
    expect(document.getElementById("project-root-hint")?.textContent).toBe("Full path, like /home/you/my-app");
  });

  it("shows a busy button while opening and clears the old error on submit", async () => {
    const core = new FailingAdd(null);
    const t = mountList(core);
    await settle();
    await t.send("relative");
    expect(t.error()).not.toBeNull();
    await t.send("/slow");
    expect(t.error()).toBeNull();
    expect(t.submit().disabled).toBe(true);
    expect(t.submit().textContent).toBe("Opening…");
    core.release();
    await settle();
    expect(t.submit().disabled).toBe(false);
    expect(t.submit().textContent).toBe("Open");
    expect(t.error()).toBe("store crashed");
  });

  it("focuses the field when there are no projects", async () => {
    const t = mountList(new FakeCore());
    await settle();
    expect(document.activeElement).toBe(t.input());
  });

  it("shows each path under a Recent heading and moves focus with arrows", async () => {
    const t = mountList(new FakeCore(undefined, structuredClone(projects)));
    await settle();
    expect(t.target.querySelector("h3")?.textContent).toBe("Recent");
    const buttons = [...t.target.querySelectorAll<HTMLButtonElement>("li button")];
    expect(buttons.map((b) => b.querySelector(".path")?.textContent)).toEqual(projects.map((p) => p.root));
    expect(buttons.map((b) => b.tabIndex)).toEqual([0, -1, -1]);
    // Nothing had focus: the first project takes it, so Enter opens it.
    expect(document.activeElement).toBe(buttons[0]);

    const key = (k: string) => {
      document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
      flushSync();
    };
    buttons[0]?.focus();
    key("ArrowDown");
    expect(document.activeElement).toBe(buttons[1]);
    key("End");
    expect(document.activeElement).toBe(buttons[2]);
    key("ArrowDown");
    expect(document.activeElement).toBe(buttons[2]);
    key("Home");
    expect(document.activeElement).toBe(buttons[0]);
    key("ArrowUp");
    expect(document.activeElement).toBe(buttons[0]);
    key("ArrowDown");
    expect(buttons.map((b) => b.tabIndex)).toEqual([-1, 0, -1]);

    buttons[1]?.click();
    await settle();
    expect(t.opened).toEqual(["p2"]);
  });
});

describe("ProjectList after a lost connection", () => {
  it("loads the list again when the core comes back", async () => {
    const core = new FakeCore();
    core.call = (() => Promise.reject(new Error("connection to core closed"))) as typeof core.call;
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(ProjectList, { target, props: { transport: core, opener: new FolderOpener(core, () => {}) } });
    cleanup = () => unmount(app);
    await settle();
    expect(target.querySelector("[role=alert]")?.textContent).toBe("connection to core closed");
    core.setStatus("closed");
    core.call = FakeCore.prototype.call.bind(core);
    core.projects = [{ id: "p1", name: "app", root: "/work/app", last_opened_ms: null, trusted: false }];
    core.setStatus("open");
    await settle();
    expect(target.textContent).toContain("/work/app");
    expect(target.querySelector("[role=alert]")).toBeNull();
  });
});

describe("ProjectList with the system's folder dialog", () => {
  class DialogCore extends FakeCore {
    async pickFolder(): Promise<string | null> {
      return "/work/picked";
    }
  }

  it("opens the picked folder from the Open folder button, which has focus", async () => {
    const t = mountList(new DialogCore());
    await settle();
    const pick = t.target.querySelector<HTMLButtonElement>("button.pick");
    expect(pick?.textContent).toBe("Open folder…Ctrl+O");

    expect(document.activeElement).toBe(pick);
    expect(t.input().labels?.[0]?.textContent).toBe("Or type its path");
    pick?.click();
    await settle();
    expect(t.opened).toEqual(["p1"]);
  });

  it("has no Open folder button without the dialog", async () => {
    const t = mountList(new FakeCore());
    await settle();
    expect(t.target.querySelector("button.pick")).toBeNull();
  });
});
