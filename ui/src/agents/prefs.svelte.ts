// The person's agents-list preferences, kept in the page's storage so they
// survive a reload. Storage errors only mean a choice lasts until the
// window closes. Settings > Agents changes them.

const KEY = "ovra.agents.prefs";

type Store = Pick<Storage, "getItem" | "setItem">;

export class AgentPrefs {
  #showResources = $state(false);

  constructor(private readonly storage: Store | null) {
    try {
      const stored: unknown = JSON.parse(storage?.getItem(KEY) ?? "null");
      if (typeof stored === "object" && stored !== null && "showResources" in stored)
        this.#showResources = stored.showResources === true;
    } catch {
      // Nothing kept, or unreadable: the default.
    }
  }

  /** Whether each agent's row shows its memory and CPU. Off by default: it distracts. */
  get showResources(): boolean {
    return this.#showResources;
  }

  set showResources(on: boolean) {
    this.#showResources = on;
    try {
      this.storage?.setItem(KEY, JSON.stringify({ showResources: on }));
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

/** The person's agents-list preferences; read and assign the fields directly. */
export const agentPrefs = new AgentPrefs(localStorageOrNull());
