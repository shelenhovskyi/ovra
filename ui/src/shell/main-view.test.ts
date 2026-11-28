import { expect, it } from "vitest";
import { MainView } from "./main-view.svelte";

it("shows one view at a time and gives focus back to where it was when it closes", () => {
  const view = new MainView();
  const before = document.createElement("button");
  const inside = document.createElement("button");
  document.body.append(before, inside);
  before.focus();
  view.show("history");
  inside.focus();
  // Another view replaces it; focus still goes back to where it was before the first one.
  view.toggle("audit");
  expect(view.current).toBe("audit");
  view.toggle("audit");
  expect(view.current).toBeNull();
  expect(document.activeElement).toBe(before);
  // Closing without refocus (a file opened) leaves focus alone.
  view.show("reviews");
  inside.focus();
  view.close(false);
  expect(document.activeElement).toBe(inside);
  before.remove();
  inside.remove();
});
