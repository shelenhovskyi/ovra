// In-app notifications (toasts) and, while Ovra is in the background, system
// notifications through the standard Web Notification API. The desktop app
// provides that API through its notification plugin, so this code never
// needs to know where it runs.

type Level = "info" | "attention" | "error";

export type Toast = {
  id: number;
  /** A later toast with the same key replaces this one; `dismissKey` removes it. */
  key?: string;
  level: Level;
  text: string;
  action?: { label: string; run: () => void };
};

type Permission = "granted" | "denied" | "default" | "unsupported";

export interface SystemNotifications {
  permission(): Permission;
  /** Asks the person; must run from a click or key press. */
  request(): Promise<Permission>;
  show(title: string, body: string, tag: string, onClick: () => void): void;
  /** Whether Ovra is the window the person looks at. */
  inForeground(): boolean;
}

/** How long an `info` toast stays. `attention` and `error` stay until dismissed or resolved. */
export const INFO_MS = 6000;
const MAX_TOASTS = 4;

export class Notifier {
  toasts = $state<Toast[]>([]);
  private nextId = 1;
  private readonly timers = new Map<number, ReturnType<typeof setTimeout>>();

  constructor(readonly system: SystemNotifications = webNotifications()) {}

  notify(toast: Omit<Toast, "id">, title = "Ovra"): void {
    if (toast.key) this.dismissKey(toast.key);
    const id = this.nextId++;
    let toasts = [...this.toasts, { ...toast, id }];
    while (toasts.length > MAX_TOASTS) {
      const oldest = toasts.find((t) => t.level === "info") ?? toasts[0];
      if (oldest) this.clearTimer(oldest.id);
      toasts = toasts.filter((t) => t !== oldest);
    }
    this.toasts = toasts;
    if (toast.level === "info") {
      this.timers.set(
        id,
        setTimeout(() => this.dismiss(id), INFO_MS),
      );
    }
    if (this.system.permission() === "granted" && !this.system.inForeground()) {
      this.system.show(title, toast.text, toast.key ?? `ovra-${id}`, () => toast.action?.run());
    }
  }

  dismiss(id: number): void {
    this.clearTimer(id);
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  dismissKey(key: string): void {
    for (const toast of this.toasts) if (toast.key === key) this.dismiss(toast.id);
  }

  private clearTimer(id: number): void {
    clearTimeout(this.timers.get(id));
    this.timers.delete(id);
  }
}

function webNotifications(): SystemNotifications {
  const api = () => (typeof Notification === "undefined" ? undefined : Notification);
  return {
    permission: () => api()?.permission ?? "unsupported",
    request: async () => {
      const notification = api();
      if (!notification) return "unsupported";
      await notification.requestPermission();
      // Read it back: the desktop app's stand-in may answer with other words.
      return notification.permission;
    },
    show: (title, body, tag, onClick) => {
      const notification = api();
      if (!notification) return;
      const shown = new notification(title, { body, tag });
      shown.onclick = () => {
        window.focus();
        onClick();
        shown.close();
      };
    },
    inForeground: () => document.visibilityState === "visible" && document.hasFocus(),
  };
}
