// Remote machines (ADR 0016) as the laptop's window sees them: the list
// from `ssh.*`, the add-machine steps, and what to say about a link. Only
// the laptop's core answers `ssh.*`.
import type { PreparedHost, RemoteHost } from "../lib/protocol";
import { CoreError, type RemoteLink, type Transport } from "../lib/transport";

const text = (err: unknown) => (err instanceof CoreError ? err.error.message : String(err));

export class Machines {
  /** `null` until the core answered, or where there are no remote machines. */
  hosts = $state<RemoteHost[] | null>(null);
  /** Why the list could not be read: this front end has no remote machines. */
  missing = $state<string | null>(null);
  error = $state<string | null>(null);

  constructor(private readonly transport: Transport) {}

  /** Loads the list and follows its changes; returns the unsubscribe. */
  follow(): () => void {
    const off = this.transport.onEvent((event) => {
      if (event.type === "ssh.changed") this.hosts = event.data;
    });
    this.transport
      .call({ method: "ssh.list" })
      .then((reply) => {
        this.hosts = reply.data;
      })
      .catch((err: unknown) => {
        this.missing = text(err);
      });
    return off;
  }

  private async attempt<T>(run: () => Promise<T>): Promise<T | null> {
    this.error = null;
    try {
      return await run();
    } catch (err) {
      this.error = text(err);
      return null;
    }
  }

  /** What `ssh` resolves for `destination` and the keys known for it; connects nowhere. */
  prepare(destination: string): Promise<PreparedHost | null> {
    return this.attempt(
      async () => (await this.transport.call({ method: "ssh.prepare", params: { destination } })).data,
    );
  }

  /** Adds the machine, trusting exactly the keys the person was shown. */
  add(name: string, prepared: PreparedHost, daemon: string): Promise<RemoteHost | null> {
    const fingerprints = prepared.keys.map((key) => key.fingerprint);
    const params = { name, destination: prepared.destination, fingerprints, ...(daemon ? { daemon } : {}) };
    return this.attempt(async () => (await this.transport.call({ method: "ssh.add", params })).data);
  }

  rename(id: string, name: string): Promise<unknown> {
    return this.attempt(() => this.transport.call({ method: "ssh.rename", params: { id, name } }));
  }

  remove(id: string): Promise<unknown> {
    return this.attempt(() => this.transport.call({ method: "ssh.remove", params: { id } }));
  }

  /** Copies this computer's build to the machine, then reads the list again for its new path; the person confirmed first. */
  async install(host: RemoteHost): Promise<boolean> {
    const done = await this.attempt(async () => {
      await this.transport.installRemote?.(host.id);
      return true;
    });
    if (!done) return false;
    // The app changed the machine's record, not the core: no `ssh.changed` came.
    const reply = await this.attempt(() => this.transport.call({ method: "ssh.list" }));
    if (reply) this.hosts = reply.data;
    return true;
  }

  /** Ends Ovra on the machine and starts its installed build; the person confirmed first. */
  restart(host: RemoteHost): Promise<unknown> {
    return this.attempt(async () => this.transport.restartRemote?.(host.id));
  }

  /** Opens the machine's window, or focuses it. */
  open(host: RemoteHost): Promise<unknown> {
    return this.attempt(async () => this.transport.openRemote?.({ id: host.id, name: host.name }));
  }
}

/** What the window says about its link to `name`, in a sentence. */
export function linkMessage(name: string, link: RemoteLink): string {
  if (link.state === "connecting") return `Connecting to ${name}…`;
  if (link.state === "open") return `Connected to ${name}.`;
  if (link.state === "stopped") return `Ovra on ${name} is stopped. Its agents' sessions can be resumed.`;
  switch (link.reason) {
    case null:
      return `Disconnected from ${name}. Agents there keep working.`;
    case "replaced":
      return `Ovra on ${name} was opened from another window or computer. Reconnect to take it back here.`;
    case "host_key_changed":
      return `${name} has a different host key than the one Ovra trusts. Ovra did not connect: someone may be posing as it. If the machine was reinstalled, remove it in Settings and add it again.`;
    case "no_pin":
      return `Ovra has no trusted key for ${name}. Remove it in Settings and add it again.`;
    case "auth_failed":
      return `${name} did not accept your SSH key. Check that \`ssh\` to it works in a terminal without a password prompt.`;
    case "no_daemon":
      return `Ovra is not installed on ${name}, or not where Ovra looked. Install it from Settings > Remote machines in Ovra's main window.`;
    case "version_mismatch":
      return `${name} runs a different version of Ovra. Install this one, or restart it, from Settings > Remote machines in Ovra's main window.`;
    case "busy":
      return `Ovra is already open on ${name}, for you or for someone else. Close it there first.`;
    case "no_ssh":
      return "Ovra could not find `ssh` on this computer. Install OpenSSH and try again.";
    case "unsafe_config":
      return `Your SSH settings for ${name} turn on something Ovra keeps off, such as forwarding. See Details.`;
    default:
      return `Disconnected from ${name}. Agents there keep working.`;
  }
}
