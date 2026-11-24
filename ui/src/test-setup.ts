// Vitest setup for every UI test file (see `test.setupFiles` in vite.config.ts).
import { vi } from "vitest";

// `vi.waitFor` polls every 50 ms by default, so a condition that is not met on
// the first check costs a whole 50 ms. Poll every millisecond instead; the
// timeout stays the same.
const waitFor = vi.waitFor;
vi.waitFor = ((callback, options) =>
  waitFor(
    callback,
    typeof options === "number" ? { timeout: options, interval: 1 } : { interval: 1, ...options },
  )) as typeof vi.waitFor;
