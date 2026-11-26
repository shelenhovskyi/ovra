import { describe, expect, it } from "vitest";
import type { Request } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { atQuery, ContextDrafts, describeItem } from "./context.svelte";

describe("atQuery", () => {
  it("finds the @ word the caret is in, and only one that starts a word", () => {
    expect(atQuery("Fix @src/li", 11)).toEqual({ start: 4, query: "src/li" });
    expect(atQuery("@", 1)).toEqual({ start: 0, query: "" });
    expect(atQuery("mail me@home", 12)).toBeNull();
    expect(atQuery("@lib then more", 14)).toBeNull();
    expect(atQuery("@lib then", 4)).toEqual({ start: 0, query: "lib" });
    // A full-width ＠, as some input methods type it.
    expect(atQuery("Fix ＠lib", 8)).toEqual({ start: 4, query: "lib" });
  });
});

describe("chips", () => {
  it("name each item briefly and keep the full name for the tooltip", () => {
    expect(describeItem({ kind: "folder", path: "ui/src/" })).toMatchObject({ label: "src/", title: "ui/src/" });
    expect(describeItem({ kind: "selection", path: "a/b.rs", from_line: 3, to_line: 3, text: "x" })).toMatchObject({
      label: "b.rs:3",
      icon: "selection",
    });
    expect(
      describeItem({ kind: "problem", path: "b.rs", line: 9, severity: "error", message: "unused" }),
    ).toMatchObject({ label: "b.rs:9", title: "b.rs:9 unused" });
    // Text items show what they send in the tooltip; a terminal is named by its title.
    const long = Array.from({ length: 8 }, (_, i) => `line ${i}`).join("\n");
    expect(describeItem({ kind: "terminal", title: "zsh", text: long })).toMatchObject({
      label: "zsh",
      title: `Output of zsh\n${long.split("\n").slice(0, 6).join("\n")}\n…`,
    });
  });

  it("are added once, marked by the core's check and sent with what the person chose", async () => {
    const calls: Request[] = [];
    const transport = {
      call: async (request: Request) => {
        calls.push(request);
        return { type: "context.checked", data: { notes: ["secret_file"] } };
      },
      onEvent: () => () => {},
    } as unknown as Transport;
    const drafts = new ContextDrafts(transport);
    await drafts.add("p1", "new:p1", { kind: "file", path: ".env" });
    await drafts.add("p1", "new:p1", { kind: "file", path: ".env" });
    expect(drafts.of("new:p1")).toHaveLength(1);
    expect(calls).toHaveLength(1);
    const key = drafts.of("new:p1")[0]?.key ?? "";
    expect(drafts.unconfirmed("new:p1")).toHaveLength(1);
    drafts.confirm("new:p1", key);
    expect(drafts.items("new:p1")).toEqual([{ kind: "file", path: ".env", confirmed: true }]);
    drafts.remove("new:p1", key);
    expect(drafts.of("new:p1")).toEqual([]);
  });
});
