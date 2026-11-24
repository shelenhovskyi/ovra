// Web Worker entry: tree-sitter parsing off the UI thread.

import { Parser } from "web-tree-sitter";
import runtime from "web-tree-sitter/web-tree-sitter.wasm?url";
import { HighlightEngine, serve } from "./engine";
import type { FromWorker, Port, ToWorker } from "./protocol";

serve(new HighlightEngine(Parser.init({ locateFile: () => runtime })), self as unknown as Port<ToWorker, FromWorker>);
