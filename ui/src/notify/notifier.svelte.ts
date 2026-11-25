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
  /** It points to Reviews: out of sight while Reviews is open. */
  review?: boolean;
};

type Permission = "granted" | "denied" | "default" | "unsupported";

export interface SystemNotifications {
  permission(): Permission;
  /** Asks the person; must run from a click or key press. */
  request(): Promise<Permission>;
  show(title: string, body: string, tag: string, onClick: () => void): void;
  /** Takes back the notification shown with this tag, if it is still there. */
  close(tag: string): void;
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
  /** Reviews is on screen: toasts that point there would only cover it. */
  private reviewsOpen = false;

  constructor(readonly system: SystemNotifications = webNotifications()) {}

  /** `system`: false only updates the toast (a count that went down), never a system notification. */
  notify(toast: Omit<Toast, "id">, title = "Ovra", system = true): void {
    if (toast.key) {
      for (const old of this.toasts) if (old.key === toast.key) this.dismiss(old.id);
      if (system) this.system.close(toast.key);
    }
    const id = this.nextId++;
    if (system && this.system.permission() === "granted" && !this.system.inForeground()) {
      this.system.show(title, toast.text, toast.key ?? `ovra-${id}`, () => toast.action?.run());
    }
    if (toast.review && this.reviewsOpen) return;
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
  }

  /** While Reviews is open, toasts that point there go, and new ones are not shown. */
  stepAsideForReviews(open: boolean): void {
    this.reviewsOpen = open;
    if (open) for (const toast of this.toasts) if (toast.review) this.dismiss(toast.id);
  }

  dismiss(id: number): void {
    this.clearTimer(id);
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /** Also takes back its system notification: what it was about is over. */
  dismissKey(key: string): void {
    for (const toast of this.toasts) if (toast.key === key) this.dismiss(toast.id);
    this.system.close(key);
  }

  private clearTimer(id: number): void {
    clearTimeout(this.timers.get(id));
    this.timers.delete(id);
  }
}

type Box = Pick<DOMRect, "top" | "bottom" | "left" | "right">;

/** Whether two boxes share any area; an empty box (nothing laid out) shares none. */
export function overlaps(a: Box, b: Box): boolean {
  const empty = (box: Box) => box.bottom <= box.top || box.right <= box.left;
  if (empty(a) || empty(b)) return false;
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

function webNotifications(): SystemNotifications {
  const api = () => (typeof Notification === "undefined" ? undefined : Notification);
  const open = new Map<string, Notification>();
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
      open.set(tag, shown);
      shown.onclose = () => {
        if (open.get(tag) === shown) open.delete(tag);
      };
      shown.onclick = () => {
        window.focus();
        onClick();
        shown.close();
      };
    },
    close: (tag) => {
      open.get(tag)?.close();
      open.delete(tag);
    },
    inForeground: () => document.visibilityState === "visible" && document.hasFocus(),
  };
}
