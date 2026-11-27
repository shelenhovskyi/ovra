import { describe, expect, it, vi } from "vitest";
import { connectTauri } from "./transport";

const tauri = vi.hoisted(() => {
  /** The window's channel the transport handed to `ovra_subscribe`. */
  const state: { channel: ((message: unknown) => void) | null } = { channel: null };
  const invoke = vi.fn(async (command: string, args?: unknown): Promise<unknown> => {
    if (command === "ovra_subscribe") {
      state.channel = (args as { channel: { onmessage: (message: unknown) => void } }).channel.onmessage;
      return null;
    }
    return "/work/picked";
  });
  return { invoke, state };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  Channel: class {
    constructor(public onmessage: (message: unknown) => void) {}
  },
}));

/** The window's label, as Tauri tells the page. */
const label = (name: string) => {
  (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: name } } };
};
label("main");

/** Sends `message` on the window's channel, as the app does. */
const send = (message: unknown) => tauri.state.channel?.(message);
describe("the Tauri transport", () => {
  it("picks a folder through the window's own command", async () => {
    const transport = connectTauri();
    expect(await transport.pickFolder?.()).toBe("/work/picked");
    expect(tauri.invoke).toHaveBeenCalledWith("ovra_pick_folder");
  });

  it("hands the app's drops to listeners, until they unsubscribe", async () => {
    tauri.state.channel = null;
    const transport = connectTauri();
    const dropped: unknown[] = [];
    const off = transport.onDrop?.((drop) => dropped.push(drop));
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    const drop = { id: "d1", paths: ["/work/app"], x: 10, y: 20 };
    send({ drop });
    off?.();
    send({ drop: { ...drop, id: "d2" } });
    expect(dropped).toEqual([drop]);
  });

  it("takes the core's frames from the window's own channel", async () => {
    tauri.state.channel = null;
    const transport = connectTauri();
    const seen: unknown[] = [];
    transport.onEvent((event) => seen.push(event));
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    send({ frame: JSON.stringify({ event: { type: "info.ready" } }) });
    // A status for the window is not a frame.
    send({ sshStatus: { id: "abc", state: "open" } });
    expect(seen).toEqual([{ type: "info.ready" }]);
  });

  it("follows a drag from outside as the app reports it, and its end", async () => {
    tauri.state.channel = null;
    const transport = connectTauri();
    const seen: unknown[] = [];
    const off = transport.onDragOver?.((at) => seen.push(at));
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    const over = (dragOver: unknown) => send({ dragOver });
    over({ x: 10, y: 20 });
    over(null);
    off?.();
    over({ x: 30, y: 40 });
    expect(seen).toEqual([{ x: 10, y: 20 }, null]);
  });

  it("imports a drop by its id through the window's own command", async () => {
    tauri.state.channel = null;
    const transport = connectTauri();
    // After the window subscribed, so the next answer is the import's.
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    tauri.invoke.mockResolvedValueOnce({ kind: "copied", paths: ["src/a.png"], skipped_links: [] });
    const done = await transport.importDrop?.("d1", "p1", "src", "ask");
    expect(done).toEqual({ kind: "copied", paths: ["src/a.png"], skipped_links: [] });
    expect(tauri.invoke).toHaveBeenCalledWith("ovra_import_drop", {
      dropId: "d1",
      project: "p1",
      targetDir: "src",
      clash: "ask",
    });
    tauri.invoke.mockRejectedValueOnce("That drop is over. Drop the files again.");
    await expect(transport.importDrop?.("d1", "p1", "src", "ask")).rejects.toThrow("That drop is over");
  });

  it("hands menu bar picks to listeners, until they unsubscribe", async () => {
    tauri.state.channel = null;
    const transport = connectTauri();
    const picked: string[] = [];
    const off = transport.onMenu?.((command) => picked.push(command));
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    send({ menu: "settings.open" });
    off?.();
    send({ menu: "palette.search" });
    expect(picked).toEqual(["settings.open"]);
  });

  it("opens a remote machine's window from the laptop's own", async () => {
    const transport = connectTauri();
    expect(transport.remote).toBeUndefined();
    await transport.openRemote?.({ id: "abc", name: "Build" });
    expect(tauri.invoke).toHaveBeenCalledWith("ovra_remote_show", { id: "abc" });
  });
});

describe("a remote machine's Tauri window", () => {
  const remoteWindow = () => {
    tauri.state.channel = null;
    (window as { __OVRA_REMOTE__?: unknown }).__OVRA_REMOTE__ = { id: "abc", name: "Build" };
    label("ssh-abc");
    try {
      return connectTauri();
    } finally {
      delete (window as { __OVRA_REMOTE__?: unknown }).__OVRA_REMOTE__;
      label("main");
    }
  };

  it("refuses to pass for the laptop's window without the machine's identity", () => {
    label("ssh-abc");
    try {
      expect(() => connectTauri()).toThrow("which machine");
    } finally {
      label("main");
    }
  });

  it("has no folder dialog, drops or remote windows of its own", () => {
    const transport = remoteWindow();
    expect(transport.remote?.machine).toEqual({ id: "abc", name: "Build" });
    expect(transport.pickFolder).toBeUndefined();
    expect(transport.onDrop).toBeUndefined();
    expect(transport.importDrop).toBeUndefined();
    expect(transport.openRemote).toBeUndefined();
  });

  it("follows its own machine's link only", async () => {
    const transport = remoteWindow();
    const seen: unknown[] = [];
    transport.remote?.onLink((link) => seen.push(link.state));
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    send({ sshStatus: { id: "abc", state: "open", reason: null, details: null } });
    send({ sshStatus: { id: "zzz", state: "failed", reason: "auth_failed", details: null } });
    send({ sshStatus: { id: "abc", state: "closed", reason: "exited", details: "bye" } });
    expect(seen).toEqual(["connecting", "open", "closed"]);
  });

  it("opens links through the app, which checks them, not through the machine", async () => {
    const transport = remoteWindow();
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    tauri.invoke.mockClear();
    tauri.invoke.mockResolvedValueOnce(null);
    expect(await transport.call({ method: "open.url", params: { url: "https://example.com" } })).toEqual({
      type: "ok",
    });
    expect(tauri.invoke).toHaveBeenCalledWith("ovra_open_url", { url: "https://example.com" });
    expect(tauri.invoke).not.toHaveBeenCalledWith("ovra_call", expect.anything());
    tauri.invoke.mockRejectedValueOnce("Ovra opens only web links that do not point at this computer.");
    await expect(transport.call({ method: "open.url", params: { url: "http://10.0.0.1" } })).rejects.toThrow(
      "only web links",
    );
  });

  it("asks the app to reconnect and disconnect its own link", async () => {
    const transport = remoteWindow();
    await vi.waitFor(() => expect(tauri.state.channel).not.toBeNull());
    transport.remote?.disconnect();
    transport.remote?.reconnect();
    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("ovra_remote_reconnect"));
    expect(tauri.invoke).toHaveBeenCalledWith("ovra_remote_disconnect");
  });
});
