import { afterEach, describe, expect, it, vi } from "vitest";
import { registerCommand } from "./commands";
import { installKeybindings } from "./keybindings";

const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

function keydown(init: KeyboardEventInit): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  document.body.dispatchEvent(event);
  return event;
}

describe("keybindings", () => {
  it("runs the command bound to the pressed shortcut and prevents the default", () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.save", title: "Save", keys: "Mod+S", run }));
    cleanups.push(installKeybindings(window, false));

    const event = keydown({ key: "s", code: "KeyS", ctrlKey: true });
    expect(run).toHaveBeenCalledOnce();
    expect(event.defaultPrevented).toBe(true);

    const plain = keydown({ key: "s", code: "KeyS" });
    expect(run).toHaveBeenCalledOnce();
    expect(plain.defaultPrevented).toBe(false);
  });

  it("leaves keys that a focused widget already handled", () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.save", title: "Save", keys: "Mod+S", run }));
    cleanups.push(installKeybindings(window, false));
    const widget = document.createElement("div");
    document.body.append(widget);
    cleanups.push(() => widget.remove());
    widget.addEventListener("keydown", (event) => event.preventDefault());
    widget.dispatchEvent(
      new KeyboardEvent("keydown", { key: "s", code: "KeyS", ctrlKey: true, bubbles: true, cancelable: true }),
    );
    expect(run).not.toHaveBeenCalled();
  });

  it("ignores malformed shortcuts and stops listening when uninstalled", () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.bad", title: "Bad", keys: "Hyper+S", run }));
    cleanups.push(registerCommand({ id: "t.ok", title: "Ok", keys: "Mod+S", run }));
    const uninstall = installKeybindings(window, false);
    uninstall();
    keydown({ key: "s", code: "KeyS", ctrlKey: true });
    expect(run).not.toHaveBeenCalled();
  });

  it("reports a failing command instead of throwing", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    cleanups.push(() => error.mockRestore());
    cleanups.push(
      registerCommand({
        id: "t.fail",
        title: "Fail",
        keys: "F2",
        run: () => Promise.reject(new Error("boom")),
      }),
    );
    cleanups.push(installKeybindings(window, false));
    keydown({ key: "F2", code: "F2" });
    await vi.waitFor(() => expect(error).toHaveBeenCalledWith("command t.fail failed", expect.any(Error)));
  });

  it("uses the browser-safe stand-in in a browser tab and the original in the desktop window", () => {
    const run = vi.fn();
    cleanups.push(registerCommand({ id: "t.close", title: "Close tab", keys: "Mod+W", run }));
    const uninstallBrowser = installKeybindings(window, false, true);
    keydown({ key: "w", code: "KeyW", ctrlKey: true });
    expect(run).not.toHaveBeenCalled();
    keydown({ key: "w", code: "KeyW", altKey: true });
    expect(run).toHaveBeenCalledOnce();
    uninstallBrowser();

    cleanups.push(installKeybindings(window, false, false));
    keydown({ key: "w", code: "KeyW", ctrlKey: true });
    expect(run).toHaveBeenCalledTimes(2);
  });
});
