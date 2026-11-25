import { expect, test } from "vitest";
import { supports } from "./actions";

test("an action applies when the server offers it, or before its capabilities are known", () => {
  const formats = { documentFormattingProvider: true, renameProvider: false };
  expect(supports(formats, "format")).toBe(true);
  expect(supports(formats, "rename")).toBe(false);
  expect(supports({}, "format")).toBe(false);
  expect(supports(null, "format")).toBe(true);
  // Problems need nothing from the server.
  expect(supports({}, "problems")).toBe(true);
});
