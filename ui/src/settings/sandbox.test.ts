import { expect, it } from "vitest";
import type { SessionInfo } from "../lib/protocol";
import { projectSandbox } from "./sandbox";

const session = (id: string, extra: Partial<SessionInfo> = {}): SessionInfo => ({
  id,
  project: "p1",
  agent: "claude",
  mode: "direct",
  status: { state: "idle" },
  title: "t",
  usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
  resumable: false,
  billing: null,
  sandbox: null,
  sandbox_reported: false,
  account: null,
  ...extra,
});

const bare = (id: string) => session(id, { sandbox_reported: true });
const boxed = (id: string) =>
  session(id, { sandbox: { name: "fake", network: "proxy", gaps: [] }, sandbox_reported: true });

it("says nothing while no session reported how it runs", () => {
  expect(projectSandbox([])).toBeNull();
  expect(projectSandbox([session("s1"), session("s2", { status: { state: "stopped" } })])).toBeNull();
});

it("says none when the newest reported session runs without a sandbox", () => {
  expect(projectSandbox([boxed("s1"), bare("s2"), session("s3")])).toBe("none");
});

it("says sandboxed when a newer sandboxed session follows an older bare one", () => {
  expect(projectSandbox([bare("s1"), boxed("s2")])).toBe("sandboxed");
});
