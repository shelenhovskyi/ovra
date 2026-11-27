// The person's agents preferences, kept in the page's storage so they
// survive a reload. Storage errors only mean a choice lasts until the
// window closes. Settings > Agents changes them.

const KEY = "ovra.agents.prefs";

/** A task text the person saved to start from again (`docs/specs/task-commands.md`). */
export type Template = { id: string; name: string; text: string };

/** How many templates are kept, and how long a name may be. */
export const MAX_TEMPLATES = 50;
export const MAX_TEMPLATE_NAME = 60;

function templatesFrom(stored: unknown): Template[] {
  if (!Array.isArray(stored)) return [];
  return stored
    .filter(
      (t): t is Template =>
        typeof t === "object" &&
        t !== null &&
        typeof t.id === "string" &&
        typeof t.name === "string" &&
        typeof t.text === "string",
    )
    .map((t) => ({ id: t.id, name: t.name, text: t.text }))
    .slice(0, MAX_TEMPLATES);
}

type Store = Pick<Storage, "getItem" | "setItem">;

export class AgentPrefs {
  #showResources = $state(false);
  #attempts = $state(false);
  #pickModel = $state(false);
  #templates = $state<Template[]>([]);
  #planFirst = $state(false);

  constructor(private readonly storage: Store | null) {
    try {
      const stored: unknown = JSON.parse(storage?.getItem(KEY) ?? "null");
      if (typeof stored === "object" && stored !== null) {
        if ("showResources" in stored) this.#showResources = stored.showResources === true;
        if ("attempts" in stored) this.#attempts = stored.attempts === true;
        if ("pickModel" in stored) this.#pickModel = stored.pickModel === true;
        if ("templates" in stored) this.#templates = templatesFrom(stored.templates);
        if ("planFirst" in stored) this.#planFirst = stored.planFirst === true;
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

  /**
   * Whether the task form offers a model for Claude's sessions. Off by
   * default: the account's own default suits most tasks.
   */
  get pickModel(): boolean {
    return this.#pickModel;
  }

  set pickModel(on: boolean) {
    this.#pickModel = on;
    this.save();
  }

  /** The person's task templates, in the order they were added. */
  get templates(): readonly Template[] {
    return this.#templates;
  }

  /** Adds a template; null when the list is full. */
  addTemplate(name: string, text: string): Template | null {
    if (this.#templates.length >= MAX_TEMPLATES) return null;
    // Only told apart within the list: no need for crypto.randomUUID, which needs a secure page.
    const template = {
      id: `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`,
      name: name.trim().slice(0, MAX_TEMPLATE_NAME),
      text,
    };
    this.#templates = [...this.#templates, template];
    this.save();
    return template;
  }

  updateTemplate(id: string, name: string, text: string): void {
    this.#templates = this.#templates.map((t) =>
      t.id === id ? { id, name: name.trim().slice(0, MAX_TEMPLATE_NAME), text } : t,
    );
    this.save();
  }

  removeTemplate(id: string): void {
    this.#templates = this.#templates.filter((t) => t.id !== id);
    this.save();
  }

  /**
   * Whether the task form offers "Plan first": the agent proposes a plan and
   * changes no file until the person approves it (`docs/specs/plan-first.md`).
   * Off by default: a secondary choice.
   */
  get planFirst(): boolean {
    return this.#planFirst;
  }

  set planFirst(on: boolean) {
    this.#planFirst = on;
    this.save();
  }

  private save(): void {
    try {
      this.storage?.setItem(
        KEY,
        JSON.stringify({
          showResources: this.#showResources,
          attempts: this.#attempts,
          pickModel: this.#pickModel,
          templates: this.#templates,
          planFirst: this.#planFirst,
        }),
      );
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
