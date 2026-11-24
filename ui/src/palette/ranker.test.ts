import { describe, expect, it } from "vitest";
import { createRanker, remoteRanker, serveRanker } from "./ranker";

describe("ranker", () => {
  it("ranks in place for short lists and counts every match", async () => {
    const ranker = createRanker(["src/main.rs", "src/lib.rs", "README.md"]);
    const result = await ranker.rank("src", 1);
    expect(result.matches).toBe(2);
    expect(result.rows).toEqual([{ index: 1, positions: [0, 1, 2] }]);
  });

  it("ranks over a message port, answering each request by id", async () => {
    const channel = new MessageChannel();
    serveRanker(channel.port1);
    channel.port1.start();
    channel.port2.start();
    const ranker = remoteRanker(channel.port2, ["alpha.ts", "beta.ts", "gamma.md"]);
    const [ts, md] = await Promise.all([ranker.rank("ts", 10), ranker.rank("md", 10)]);
    expect(ts.rows.map((r) => r.index)).toEqual([1, 0]); // shorter first on equal scores
    expect(md.rows.map((r) => r.index)).toEqual([2]);
    channel.port1.close();
    channel.port2.close();
  });
});
