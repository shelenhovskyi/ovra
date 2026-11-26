import { expect, it } from "vitest";
import type { CoreEvent, PowerState, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { describe, KeepAwake } from "./keep-awake.svelte";

function fake(fail = false) {
  let state: PowerState = { mode: "off", awake: false };
  const listeners: ((e: CoreEvent) => void)[] = [];
  const sent: string[] = [];
  const transport: Transport = {
    call: ((request: Request) => {
      if (request.method === "power.get") return Promise.resolve({ type: "power.state", data: state });
      if (request.method === "power.set_mode") {
        sent.push(request.params.mode);
        if (fail) return Promise.reject(new CoreError({ code: "internal", message: "nope" }));
        state = { mode: request.params.mode, awake: request.params.mode === "always" };
        return Promise.resolve({ type: "power.state", data: state });
      }
      return Promise.reject(new Error(`unexpected ${request.method}`));
    }) as Transport["call"],
    onEvent: (listener) => {
      listeners.push(listener);
      return () => listeners.splice(listeners.indexOf(listener), 1);
    },
  };
  const emit = (e: CoreEvent) => {
    for (const listener of listeners) listener(e);
  };
  return { transport, sent, emit };
}

it("loads the state, follows changes and toggles between off and while agents work", async () => {
  const core = fake();
  const power = new KeepAwake(core.transport);
  const off = power.follow();
  await Promise.resolve();
  expect(power.state?.mode).toBe("off");

  await power.toggle();
  expect(core.sent).toEqual(["while_agents_work"]);
  core.emit({ type: "power.changed", data: { mode: "while_agents_work", awake: true } });
  expect(power.state?.awake).toBe(true);
  await power.toggle();
  expect(core.sent).toEqual(["while_agents_work", "off"]);

  off();
  core.emit({ type: "power.changed", data: { mode: "always", awake: true } });
  expect(power.state?.mode).toBe("off");
});

it("shows the core's error and keeps the last state", async () => {
  const core = fake(true);
  const power = new KeepAwake(core.transport);
  power.follow();
  await Promise.resolve();
  await power.set("always");
  expect(power.error).toBe("nope");
  expect(power.state?.mode).toBe("off");
});

it("says why the computer cannot be kept awake", () => {
  expect(describe({ mode: "always", awake: false, problem: "no logind" })).toBe(
    "Can't keep the computer awake: no logind",
  );
  expect(describe({ mode: "while_agents_work", awake: false })).toBe("Keeps the computer awake while agents work");
});
