// Claude accounts: Claude Code configuration folders with their own sign-in
// (personal, work, ...). One instance per transport, shared by the setup
// card and the account picker of the start form (`claudeAccounts(transport)`).

import type { AccountState, AccountsOverview, ClaudeAccount, ProjectId } from "../../lib/protocol";
import type { Transport } from "../../lib/transport";

/** The id of the account that runs claude as the user set it up. */
export const DEFAULT_ACCOUNT = "default";

export class ClaudeAccounts {
  overview = $state<AccountsOverview | null>(null);
  /** The project `overview.project_account` belongs to. */
  private project: ProjectId | null = null;
  private pending: { project: ProjectId | null; done: Promise<void> } | null = null;

  constructor(private readonly transport: Transport) {}

  get list(): AccountState[] {
    return this.overview?.accounts ?? [];
  }

  /** The id a new session of the loaded project starts with. */
  get projectAccount(): string {
    return this.overview?.project_account ?? DEFAULT_ACCOUNT;
  }

  /** Checks every account; `project`'s account comes with it. */
  async load(project: ProjectId | null = this.project): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.accounts", params: { project } });
    this.project = project;
    this.overview = data;
  }

  /** Loads for `project` unless that is loaded or loading already: the
   * setup card and the start form both ask, and each check runs claude. */
  ensure(project: ProjectId | null): Promise<void> {
    if (this.pending?.project === project) return this.pending.done;
    if (this.overview && this.project === project) return Promise.resolve();
    const done = this.load(project).finally(() => {
      if (this.pending?.done === done) this.pending = null;
    });
    this.pending = { project, done };
    return done;
  }

  /** Adds (`id: ""`) or changes an account, then checks them again. */
  async save(account: ClaudeAccount): Promise<void> {
    await this.transport.call({ method: "agents.save_account", params: { account } });
    await this.load();
  }

  async remove(id: string): Promise<void> {
    await this.transport.call({ method: "agents.remove_account", params: { id } });
    await this.load();
  }

  /** Makes `id` the account `project`'s sessions start with. */
  async setProject(project: ProjectId, id: string): Promise<void> {
    const account = id === DEFAULT_ACCOUNT ? null : id;
    await this.transport.call({ method: "agents.set_project_account", params: { project, account } });
    if (this.overview && this.project === project) this.overview = { ...this.overview, project_account: account };
  }

  /** The account's name; `null` is the default account. */
  name(id: string | null): string {
    const wanted = id ?? DEFAULT_ACCOUNT;
    return this.list.find((a) => a.account.id === wanted)?.account.name ?? (id ? "a removed account" : "Default");
  }
}

const instances = new WeakMap<Transport, ClaudeAccounts>();

export function claudeAccounts(transport: Transport): ClaudeAccounts {
  let accounts = instances.get(transport);
  if (!accounts) {
    accounts = new ClaudeAccounts(transport);
    instances.set(transport, accounts);
  }
  return accounts;
}
