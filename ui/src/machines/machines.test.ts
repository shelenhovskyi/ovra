import { type Component, flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { CoreEvent, PreparedHost, RemoteHost, Request } from "../lib/protocol";
import { CoreError, type RemoteLink, type RemoteWindow, type Transport } from "../lib/transport";
import LinkBanner from "./LinkBanner.svelte";
import { linkMessage } from "./machines.svelte";
import RemoteChip from "./RemoteChip.svelte";
import TestHost from "./TestHost.svelte";

const FINGERPRINT = `SHA256:${"A".repeat(43)}`;
const PREPARED: PreparedHost = {
  destination: "build",
  host_name: "build.example",
  port: 22,
  user: "dev",
  key_name: "build.example",
  keys: [{ kind: "ssh-ed25519", fingerprint: FINGERPRINT }],
};
const BUILD: RemoteHost = {
  id: "abc",
  name: "Build",
  destination: "build",
  daemon: "ovra-daemon",
  fingerprints: [FINGERPRINT],
};

function fake(options: { keys?: boolean; list?: boolean; links?: Record<string, RemoteLink> } = {}) {
  let hosts: RemoteHost[] = [];
  const listeners: ((e: CoreEvent) => void)[] = [];
  const sent: Request[] = [];
  const opened: unknown[] = [];
  const restarted: string[] = [];
  const emit = (e: CoreEvent) => {
    for (const listener of listeners) listener(e);
  };
  const transport: Transport = {
    call: ((request: Request) => {
      sent.push(request);
      switch (request.method) {
        case "ssh.list":
          return options.list === false
            ? Promise.reject(new CoreError({ code: "unavailable", message: "no remote machines here" }))
            : Promise.resolve({ type: "ssh.hosts", data: hosts });
        case "ssh.prepare":
          return Promise.resolve({
            type: "ssh.prepared",
            data: options.keys === false ? { ...PREPARED, keys: [] } : PREPARED,
          });
        case "ssh.add":
          hosts = [{ ...BUILD, name: request.params.name }];
          emit({ type: "ssh.changed", data: hosts });
          return Promise.resolve({ type: "ssh.host", data: hosts[0] });
        case "ssh.remove":
          hosts = [];
          emit({ type: "ssh.changed", data: hosts });
          return Promise.resolve({ type: "ok" });
        default:
          return Promise.reject(new Error(`unexpected ${request.method}`));
      }
    }) as Transport["call"],
    onEvent: (listener) => {
      listeners.push(listener);
      return () => listeners.splice(listeners.indexOf(listener), 1);
    },
    openRemote: async (machine) => {
      opened.push(machine);
    },
    onRemoteLinks: (listener) => {
      for (const [id, link] of Object.entries(options.links ?? {})) listener(id, link);
      return () => {};
    },
    restartRemote: async (id) => {
      restarted.push(id);
    },
  };
  if (options.links) hosts = [BUILD];
  return { transport, sent, opened, restarted };
}

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

function show<P extends Record<string, unknown>>(component: Component<P>, props: P): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(component, { target, props });
  cleanup.push(() => {
    unmount(app);
    target.remove();
  });
  return target;
}

const button = (root: HTMLElement, text: string) =>
  [...root.querySelectorAll("button")].find((b) => b.textContent?.trim() === text) as HTMLButtonElement;

function type(input: Element | null, value: string): void {
  const field = input as HTMLInputElement;
  field.value = value;
  field.dispatchEvent(new Event("input"));
}

it("adds a machine only after showing the keys it will trust, then opens it", async () => {
  const core = fake();
  const root = show(TestHost, { transport: core.transport });
  await settle();
  button(root, "Add a machine").click();
  flushSync();
  const inputs = root.querySelectorAll("[data-testid=machine-form] input");
  type(inputs[0] ?? null, "Build");
  type(inputs[1] ?? null, "build");
  button(root, "Next").click();
  await settle();
  // Nothing is trusted yet: the person sees where it goes and its key first.
  expect(core.sent.map((r) => r.method)).toEqual(["ssh.list", "ssh.prepare"]);
  const keys = root.querySelector("[data-testid=machine-keys]")?.textContent ?? "";
  expect(keys).toContain("dev@build.example:22");
  expect(keys).toContain(FINGERPRINT);
  button(root, "Trust and add").click();
  await settle();
  expect(core.sent.at(-1)).toEqual({
    method: "ssh.add",
    params: { name: "Build", destination: "build", fingerprints: [FINGERPRINT] },
  });
  expect(root.querySelector("[data-testid=machines-list]")?.textContent).toContain("Build");
  button(root, "Open").click();
  await settle();
  expect(core.opened).toEqual([{ id: "abc", name: "Build" }]);
});

it("tells the person to connect once in a terminal when no key is known", async () => {
  const core = fake({ keys: false });
  const root = show(TestHost, { transport: core.transport });
  await settle();
  button(root, "Add a machine").click();
  flushSync();
  type(root.querySelectorAll("[data-testid=machine-form] input")[1] ?? null, "build");
  button(root, "Next").click();
  await settle();
  expect(root.querySelector("[role=alert]")?.textContent).toContain("ssh build");
  expect(button(root, "Trust and add")).toBeUndefined();
});

it("offers to restart a machine that runs another version, after the person confirms", async () => {
  const links = { abc: { state: "failed", reason: "version_mismatch", details: null } as RemoteLink };
  const core = fake({ links });
  const root = show(TestHost, { transport: core.transport });
  await settle();
  expect(root.querySelector("[data-testid=machine-other-version]")?.textContent).toContain("different version");
  button(root, "Restart Ovra on Build").click();
  flushSync();
  expect(root.querySelector("[data-testid=machine-other-version]")?.textContent).toContain("Agents running there stop");
  expect(core.restarted).toEqual([]);
  button(root, "Restart").click();
  await settle();
  expect(core.restarted).toEqual(["abc"]);
});

it("offers no restart for a machine whose link is fine", async () => {
  const core = fake({ links: { abc: { state: "closed", reason: null, details: null } } });
  const root = show(TestHost, { transport: core.transport });
  await settle();
  expect(root.querySelector("[data-testid=machine-other-version]")).toBeNull();
});

it("says so where there are no remote machines", async () => {
  const root = show(TestHost, { transport: fake({ list: false }).transport });
  await settle();
  expect(root.querySelector("[data-testid=machines-missing]")).not.toBeNull();
  expect(button(root, "Add a machine")).toBeUndefined();
});

function remoteWindow() {
  const listeners = new Set<(link: RemoteLink) => void>();
  let link: RemoteLink = { state: "connecting", reason: null, details: null };
  const asked: string[] = [];
  const remote: RemoteWindow = {
    machine: { id: "abc", name: "Build" },
    onLink: (listener) => {
      listeners.add(listener);
      listener(link);
      return () => listeners.delete(listener);
    },
    reconnect: () => asked.push("reconnect"),
    disconnect: () => asked.push("disconnect"),
    stop: () => asked.push("stop"),
  };
  const set = (next: RemoteLink) => {
    link = next;
    for (const listener of listeners) listener(next);
    flushSync();
  };
  return { remote, set, asked };
}

it("names the machine in the title bar and disconnects from it", () => {
  const win = remoteWindow();
  const root = show(RemoteChip, { remote: win.remote });
  flushSync();
  win.set({ state: "open", reason: null, details: null });
  const chip = root.querySelector("[data-testid=remote-chip]");
  expect(chip?.textContent).toContain("on Build");
  expect(chip?.getAttribute("data-state")).toBe("open");
  button(root, "Disconnect").click();
  expect(win.asked).toEqual(["disconnect"]);
  win.set({ state: "closed", reason: null, details: null });
  expect(button(root, "Disconnect")).toBeUndefined();
});

it("stops Ovra on the machine only after the person confirms", () => {
  const win = remoteWindow();
  const root = show(RemoteChip, { remote: win.remote });
  win.set({ state: "open", reason: null, details: null });
  const stop = root.querySelector<HTMLButtonElement>("[data-testid=remote-stop]");
  expect(stop?.getAttribute("aria-label")).toBe("Stop Ovra on Build");
  stop?.click();
  flushSync();
  expect(root.querySelector("[role=dialog]")?.textContent).toContain("Agents running there stop");
  button(root, "Cancel").click();
  flushSync();
  expect(win.asked).toEqual([]);
  stop?.click();
  flushSync();
  button(root, "Stop Ovra").click();
  flushSync();
  expect(win.asked).toEqual(["stop"]);
  expect(root.querySelector("[role=dialog]")).toBeNull();
});

it("shows a dropped link with the machine's own words as plain text, and reconnects", () => {
  const win = remoteWindow();
  const root = show(LinkBanner, { remote: win.remote });
  flushSync();
  expect(root.querySelector("[data-testid=remote-banner]")).toBeNull();
  win.set({ state: "failed", reason: "auth_failed", details: "<img src=x onerror=alert(1)> denied" });
  const banner = root.querySelector("[data-testid=remote-banner]");
  expect(banner?.querySelector("p")?.textContent).toBe(
    linkMessage("Build", { state: "failed", reason: "auth_failed", details: null }),
  );
  expect(banner?.querySelector("pre")?.textContent).toBe("<img src=x onerror=alert(1)> denied");
  expect(banner?.querySelector("img")).toBeNull();
  button(root, "Reconnect").click();
  expect(win.asked).toEqual(["reconnect"]);
  win.set({ state: "open", reason: null, details: null });
  expect(root.querySelector("[data-testid=remote-banner]")).toBeNull();
});

it("words every reason in Ovra's own sentences", () => {
  const down = (reason: string | null) => linkMessage("Build", { state: "closed", reason, details: null });
  expect(down(null)).toBe("Disconnected from Build. Agents there keep working.");
  expect(down("replaced")).toContain("opened from another window or computer");
  expect(linkMessage("Build", { state: "stopped", reason: null, details: null })).toContain("is stopped");
  expect(down("host_key_changed")).toContain("different host key");
  expect(down("busy")).toContain("already open on Build");
  // A reason Ovra does not know is not shown as given.
  expect(down("<b>pwned</b>")).toBe(down(null));
});
