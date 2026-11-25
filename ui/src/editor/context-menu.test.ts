import { expect, test } from "vitest";
import { absolutePath } from "./context-menu";

test("an absolute path is written the way the project root is", () => {
  expect(absolutePath("/home/me/app", "src/main.rs")).toBe("/home/me/app/src/main.rs");
  expect(absolutePath("/home/me/app/", "a.rs")).toBe("/home/me/app/a.rs");
  expect(absolutePath("C:\\Users\\me\\app", "src/main.rs")).toBe("C:\\Users\\me\\app\\src\\main.rs");
});
