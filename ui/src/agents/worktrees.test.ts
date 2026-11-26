import { describe, expect, it } from "vitest";
import type { SetupProgress } from "../lib/protocol";
import { diskLabel } from "./resources";
import { diskTotal, folderName, preparingLabel, stepLine, waitingReason } from "./worktrees";

const GB = 2 ** 30;

const setup = (over: Partial<SetupProgress> = {}): SetupProgress => ({
  step: 1,
  steps: 3,
  command: "pnpm install",
  log: [],
  failed: null,
  ...over,
});

describe("worktrees", () => {
  it("says a size is an upper bound, or that counting stopped early", () => {
    expect(diskLabel({ bytes: 1.2 * GB, complete: true })).toBe("up to 1.2 GB on disk");
    expect(diskLabel({ bytes: 1.2 * GB, complete: false })).toBe("more than 1.2 GB on disk");
  });

  it("adds sizes up, and a part counted in part makes the sum a lower bound", () => {
    expect(diskTotal([null, undefined])).toBeNull();
    expect(diskTotal([{ bytes: GB, complete: true }, null, { bytes: GB, complete: true }])).toEqual({
      bytes: 2 * GB,
      complete: true,
    });
    expect(
      diskTotal([
        { bytes: GB, complete: true },
        { bytes: GB, complete: false },
      ])?.complete,
    ).toBe(false);
  });

  it("names the step from one, of how many", () => {
    expect(stepLine(setup())).toBe("2 of 3: pnpm install");
    expect(preparingLabel(null)).toBe("Preparing worktree");
    expect(preparingLabel(setup())).toBe("Preparing worktree · 2 of 3: pnpm install");
    expect(preparingLabel(setup({ failed: "exited with code 1" }))).toBe("Setup failed at 2 of 3: pnpm install");
  });

  it("says why a worker waits in a few words", () => {
    expect(
      waitingReason(
        "waits for disk space: the volume of Ovra's data folder has 3.1 GiB free, and a new worktree needs at least 5 GiB",
      ),
    ).toBe("Waiting for disk space");
    expect(waitingReason("the disk is full")).toBe("Waiting: the disk is full");
  });

  it("shows a folder by its name", () => {
    expect(folderName("/data/worktrees/p1/old-docs/")).toBe("old-docs");
    expect(folderName("C:\\data\\worktrees\\x")).toBe("x");
  });
});
