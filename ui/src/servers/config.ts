// Adding a server to `.ovra/servers.json` from the panel's form. The file
// is read, the definition appended and the whole file written back with
// `fs.write_file`, as the editor saves it; the core checks the file again
// whenever it lists or starts servers.

import type { ProjectId, ServerDef } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export const CONFIG = ".ovra/servers.json";

/** The port the sandbox reaches its network proxy on; no server may use it. */
const PROXY_BRIDGE_PORT = 3128;

export type ServerForm = { name: string; command: string; folder: string; port: string };

export type FormCheck = { def: ServerDef } | { field: keyof ServerForm; error: string };

/** The definition the form describes, or what to fix first; mirrors the core's check. */
export function checkForm(form: ServerForm, defined: ServerDef[]): FormCheck {
  const name = form.name.trim();
  if (!/^[a-z0-9][a-z0-9-]{0,31}$/.test(name))
    return { field: "name", error: "Name it with lowercase letters, digits and dashes, such as web." };
  if (defined.some((d) => d.name === name)) return { field: "name", error: `A server named ${name} exists already.` };
  const command = form.command.trim().split(/\s+/).filter(Boolean);
  if (command.length === 0) return { field: "command", error: "Type the command that starts it, such as pnpm dev." };
  const folder = form.folder.trim().replace(/\/+$/, "");
  const parts = folder.split(/[/\\]/);
  if (folder !== "" && (/^[/\\]/.test(folder) || folder.includes(":") || parts.some((p) => p === "" || p === "..")))
    return { field: "folder", error: "Write the folder inside the project, such as ui, or leave it empty." };
  const port = Number(form.port.trim());
  if (!Number.isInteger(port) || port < 1024 || port > 65535 || port === PROXY_BRIDGE_PORT)
    return { field: "port", error: `Use a port from 1024 to 65535, except ${PROXY_BRIDGE_PORT}.` };
  if (defined.some((d) => d.port === port)) return { field: "port", error: `Another server uses port ${port}.` };
  const def: ServerDef = { name, command, port };
  if (folder !== "" && folder !== ".") def.cwd = folder;
  return { def };
}

/** Appends `def` to the project's servers file, creating it if there is none. */
export async function addServer(transport: Transport, project: ProjectId, def: ServerDef): Promise<void> {
  let file: { servers: unknown[] } & Record<string, unknown> = { servers: [] };
  let mtime: number | null = null;
  try {
    const { data } = await transport.call({ method: "fs.read_file", params: { project, path: CONFIG } });
    if (data.body.kind !== "text") throw new Error(`${CONFIG} is not a text file. Open it to fix it.`);
    file = parse(data.body.text);
    mtime = data.mtime_ms;
  } catch (err) {
    if (!(err instanceof CoreError && err.code === "not_found")) throw err;
  }
  const text = `${JSON.stringify({ ...file, servers: [...file.servers, def] }, null, 2)}\n`;
  // With the time it was read, a change made meanwhile is refused, not overwritten.
  await transport.call({ method: "fs.write_file", params: { project, path: CONFIG, text, expected_mtime_ms: mtime } });
}

/** Writes `text` as the servers file unless there is one already. */
export async function createConfig(transport: Transport, project: ProjectId, text: string): Promise<void> {
  try {
    await transport.call({ method: "fs.read_file", params: { project, path: CONFIG } });
    return;
  } catch (err) {
    if (!(err instanceof CoreError && err.code === "not_found")) throw err;
  }
  await transport.call({ method: "fs.write_file", params: { project, path: CONFIG, text, expected_mtime_ms: null } });
}

function parse(text: string): { servers: unknown[] } & Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    value = null;
  }
  if (value && typeof value === "object" && Array.isArray((value as { servers?: unknown }).servers))
    return value as { servers: unknown[] };
  throw new Error(`${CONFIG} is not a list of servers Ovra can add to. Open it to fix it.`);
}
