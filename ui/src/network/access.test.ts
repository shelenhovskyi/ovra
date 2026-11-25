import { describe, expect, it } from "vitest";
import { ProjectAccess } from "./access.svelte";
import { FakeAccessCore } from "./fake-access";

describe("ProjectAccess", () => {
  it("keeps only the newest project's answer and clears on null", async () => {
    const core = new FakeAccessCore();
    core.hosts = [{ host: "pypi.org" }];
    const access = new ProjectAccess(core);
    const first = access.show("p1");
    const second = access.show(null);
    await Promise.all([first, second]);
    expect(access.project).toBeNull();
    expect(access.allowlist).toBeNull();
    expect(access.loading).toBe(false);
    access.dispose();
  });

  it("reports a failed read and ignores events for other projects", async () => {
    const core = new FakeAccessCore();
    const access = new ProjectAccess(core);
    await access.show("p1");
    const before = core.calls.length;
    core.emit({ type: "secrets.changed", data: { project: "other" } });
    expect(core.calls.length).toBe(before);
    core.refuse.set("secrets.list", "core went away");
    await access.reload();
    expect(access.error).toBe("core went away");
    access.dispose();
  });
});
