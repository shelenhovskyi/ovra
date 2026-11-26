// The person's agents preferences, kept in the page's storage so they
// survive a reload. Storage errors only mean a choice lasts until the
// window closes. Settings > Agents changes them.

const KEY = "ovra.agents.prefs";

type Store = Pick<Storage, "getItem" | "setItem">;

export class AgentPrefs {
  #showResources = $state(false);
  #attempts = $state(false);

  constructor(private readonly storage: Store | null) {
    try {
      const stored: unknown = JSON.parse(storage?.getItem(KEY) ?? "null");
      if (typeof stored === "object" && stored !== null) {
        if ("showResources" in stored) this.#showResources = stored.showResources === true;
        if ("attempts" in stored) this.#attempts = stored.attempts === true;
      }
    } catch {
      // Nothing kept, or unreadable: the defaults.
    }
  }

  /** Whether each agent's row shows its memory and CPU. Off by default: it distracts. */
  get showResources(): boolean {
    return this.#showResources;
  }

  set showResources(on: boolean) {
    this.#showResources = on;
    this.save();
  }

  /**
   * Whether the task form offers several attempts at one task, each agent in
   * its own copy (`docs/specs/attempts.md`). Off by default: most tasks need one.
   */
  get attempts(): boolean {
    return this.#attempts;
  }

  set attempts(on: boolean) {
    this.#attempts = on;
    this.save();
  }

  private save(): void {
    try {
      this.storage?.setItem(KEY, JSON.stringify({ showResources: this.#showResources, attempts: this.#attempts }));
    } catch {
      // Quota or disabled storage: the choice lasts as long as the page.
    }
  }
}

function localStorageOrNull(): Store | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** The person's agents preferences; read and assign the fields directly. */
export const agentPrefs = new AgentPrefs(localStorageOrNull());
