// URLs of the tree-sitter grammars named by `languages/*/language.json`.
// Imports must be static so Vite bundles each `.wasm` as an asset.

import css from "tree-sitter-css/tree-sitter-css.wasm?url";
import javascript from "tree-sitter-javascript/tree-sitter-javascript.wasm?url";
import json from "tree-sitter-json/tree-sitter-json.wasm?url";
import python from "tree-sitter-python/tree-sitter-python.wasm?url";
import rust from "tree-sitter-rust/tree-sitter-rust.wasm?url";
import tsx from "tree-sitter-typescript/tree-sitter-tsx.wasm?url";
import typescript from "tree-sitter-typescript/tree-sitter-typescript.wasm?url";

export const grammarUrls: Readonly<Record<string, string>> = {
  "tree-sitter-css/tree-sitter-css.wasm": css,
  "tree-sitter-javascript/tree-sitter-javascript.wasm": javascript,
  "tree-sitter-json/tree-sitter-json.wasm": json,
  "tree-sitter-python/tree-sitter-python.wasm": python,
  "tree-sitter-rust/tree-sitter-rust.wasm": rust,
  "tree-sitter-typescript/tree-sitter-tsx.wasm": tsx,
  "tree-sitter-typescript/tree-sitter-typescript.wasm": typescript,
};
