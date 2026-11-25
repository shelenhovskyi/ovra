// Turning what the person types into allowlist entries and back.

import type { AllowedHost } from "../lib/protocol";

/** `host` or `host:port` as typed, or why it cannot be one. The core checks the rest. */
export function parseHost(text: string): AllowedHost | { error: string } {
  const typed = text.trim().toLowerCase();
  if (!typed) return { error: "Type a host name, such as registry.npmjs.org." };
  if (/[\s/@*]/.test(typed) || typed.includes("://")) {
    return { error: "Type only the host name, without a scheme, path or wildcard." };
  }
  const colon = typed.lastIndexOf(":");
  if (colon < 0) return { host: typed };
  const port = Number(typed.slice(colon + 1));
  if (!/^\d{1,5}$/.test(typed.slice(colon + 1)) || port < 1 || port > 65535) {
    return { error: "The port is a number from 1 to 65535." };
  }
  return { host: typed.slice(0, colon), port };
}

/** How an entry reads: `host`, or `host:port`. */
export function hostLabel(entry: AllowedHost): string {
  return entry.port ? `${entry.host}:${entry.port}` : entry.host;
}

/** Whether `allowed` lets a request reach `host` (`name` or `name:port`, 443 by default). */
export function reaches(allowed: AllowedHost[], host: string): boolean {
  const [name, port] = host.includes(":") ? host.split(":") : [host, "443"];
  return allowed.some(
    (entry) => entry.host === name && (entry.port ? String(entry.port) === port : port === "443" || port === "80"),
  );
}
