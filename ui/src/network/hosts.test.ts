import { describe, expect, it } from "vitest";
import { hostLabel, parseHost, reaches } from "./hosts";

describe("parseHost", () => {
  it("reads a host, with or without a port", () => {
    expect(parseHost(" Registry.NPMjs.org ")).toEqual({ host: "registry.npmjs.org" });
    expect(parseHost("git.corp.example:8443")).toEqual({ host: "git.corp.example", port: 8443 });
  });

  it("refuses what is not one host", () => {
    for (const typed of [
      "",
      "https://a.com",
      "a.com/x",
      "*.a.com",
      "a b",
      "u@a.com",
      "a.com:0",
      "a.com:x",
      "a.com:70000",
    ]) {
      expect(parseHost(typed), typed).toHaveProperty("error");
    }
  });
});

describe("hosts", () => {
  it("labels entries and tells whether a bound host is reachable", () => {
    expect(hostLabel({ host: "a.com" })).toBe("a.com");
    expect(hostLabel({ host: "a.com", port: 8443 })).toBe("a.com:8443");
    expect(reaches([{ host: "api.github.com" }], "api.github.com")).toBe(true);
    expect(reaches([{ host: "api.github.com" }], "api.github.com:8443")).toBe(false);
    expect(reaches([{ host: "a.com", port: 8443 }], "a.com:8443")).toBe(true);
    expect(reaches([], "a.com")).toBe(false);
  });
});
