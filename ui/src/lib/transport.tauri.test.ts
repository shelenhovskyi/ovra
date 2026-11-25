import { describe, expect, it, vi } from "vitest";
import { connectTauri } from "./transport";

type DragDrop = { payload: { type: string; paths?: string[] } };

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(async (_command: string, _args?: unknown): Promise<unknown> => "/work/picked"),
  dragDrop: null as ((event: DragDrop) => void) | null,
  events: new Map<string, (event: { payload: string }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: (event: { payload: string }) => void) => {
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

  it("hands only drops to listeners, until they unsubscribe", async () => {
    tauri.dragDrop = null;
    const transport = connectTauri();
    const dropped: string[][] = [];
    const off = transport.onDrop?.((paths) => dropped.push(paths));
    await vi.waitFor(() => expect(tauri.dragDrop).not.toBeNull());
    const dragDrop = (event: DragDrop) => tauri.dragDrop?.(event);
    dragDrop({ payload: { type: "over" } });
    dragDrop({ payload: { type: "drop", paths: ["/work/app"] } });
    off?.();
    dragDrop({ payload: { type: "drop", paths: ["/work/late"] } });
    expect(dropped).toEqual([["/work/app"]]);
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
