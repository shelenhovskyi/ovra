import { describe, expect, it, vi } from "vitest";
import { connectTauri } from "./transport";

type DragDrop = { payload: { type: string; position?: { x: number; y: number } } };

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(async (_command: string, _args?: unknown): Promise<unknown> => "/work/picked"),
  dragDrop: null as ((event: DragDrop) => void) | null,
  events: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.events.set(name, handler);
    return () => {};
  },
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (handler: (event: DragDrop) => void) => {
      tauri.dragDrop = handler;
      return () => {};
    },
  }),
}));

describe("the Tauri transport", () => {
  it("picks a folder through the window's own command", async () => {
    const transport = connectTauri();
    expect(await transport.pickFolder?.()).toBe("/work/picked");
    expect(tauri.invoke).toHaveBeenCalledWith("ovra_pick_folder");
  });

  it("hands the app's drops to listeners, until they unsubscribe", async () => {
    tauri.events.clear();
    const transport = connectTauri();
    const dropped: unknown[] = [];
    const off = transport.onDrop?.((drop) => dropped.push(drop));
    await vi.waitFor(() => expect(tauri.events.has("ovra://drop")).toBe(true));
    const drop = { id: "d1", paths: ["/work/app"], x: 10, y: 20 };
    tauri.events.get("ovra://drop")?.({ payload: drop });
    off?.();
    tauri.events.get("ovra://drop")?.({ payload: { ...drop, id: "d2" } });
    expect(dropped).toEqual([drop]);
  });

  it("follows a drag from outside in CSS pixels, and its end", async () => {
    tauri.dragDrop = null;
    const transport = connectTauri();
    const seen: unknown[] = [];
    transport.onDragOver?.((at) => seen.push(at));
    await vi.waitFor(() => expect(tauri.dragDrop).not.toBeNull());
    const dragDrop = (event: DragDrop) => tauri.dragDrop?.(event);
    window.devicePixelRatio = 2;
    dragDrop({ payload: { type: "enter", position: { x: 20, y: 40 } } });
    dragDrop({ payload: { type: "over", position: { x: 30, y: 40 } } });
    dragDrop({ payload: { type: "leave" } });
    window.devicePixelRatio = 1;
    expect(seen).toEqual([{ x: 10, y: 20 }, { x: 15, y: 20 }, null]);
  });

  it("imports a drop by its id through the window's own command", async () => {
    tauri.invoke.mockResolvedValueOnce({ kind: "copied", paths: ["src/a.png"], skipped_links: [] });
    const transport = connectTauri();
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
    tauri.events.clear();
    const transport = connectTauri();
    const picked: string[] = [];
    const off = transport.onMenu?.((command) => picked.push(command));
    await vi.waitFor(() => expect(tauri.events.has("ovra://menu")).toBe(true));
    tauri.events.get("ovra://menu")?.({ payload: "settings.open" });
    off?.();
    tauri.events.get("ovra://menu")?.({ payload: "palette.search" });
    expect(picked).toEqual(["settings.open"]);
  });
});
