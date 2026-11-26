import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { expect, test } from "vitest";
import { runSearch, searchPhrases } from "./search";

test("find, replace and go to line speak in sentence case, and the line field has a name", () => {
  const view = new EditorView({
    state: EditorState.create({ doc: "one\ntwo", extensions: searchPhrases }),
    parent: document.body,
  });
  runSearch("replace", view);
  const words = (selector: string) =>
    [...view.dom.querySelectorAll(selector)].map((element) => element.textContent?.trim());
  expect(words(".cm-search button")).toEqual(["Next", "Previous", "All", "Replace", "Replace all", "×"]);
  expect(words(".cm-search label")).toEqual(["Match case", "Regex", "By word"]);
  expect(view.dom.querySelector(".cm-search [name=close]")?.getAttribute("aria-label")).toBe("Close");

  runSearch("go_to_line", view);
  const field = view.dom.querySelector<HTMLInputElement>(".cm-goto-line input");
  expect(field?.getAttribute("aria-label")).toBe("Go to line");
  expect(words(".cm-goto-line button[type=submit]")).toEqual(["Go"]);
  view.destroy();
});
