// Web Worker entry: palette ranking off the UI thread (see ranker.ts).

import { type Port, serveRanker } from "./ranker";

serveRanker(self as unknown as Port);
