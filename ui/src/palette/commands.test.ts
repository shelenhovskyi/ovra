import { describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import { registerPaletteCommands } from "./commands";

describe("palette commands", () => {
  it("palette.show opens the palette on Mod+Shift+P", async () => {
    const workspace = { paletteOpen: false };
    const unregister = registerPaletteCommands(workspace);
    const show = listCommands().find((c) => c.id === "palette.show");
    expect(show?.keys).toBe("Mod+Shift+P");
    await show?.run();
    expect(workspace.paletteOpen).toBe(true);
    unregister();
    expect(listCommands().find((c) => c.id === "palette.show")).toBeUndefined();
  });
});
