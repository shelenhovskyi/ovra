// Notifications through the browser's push service (spec section 6). A push
// carries only a kind and an opaque id, so the push service and the lock
// screen learn nothing about the work; the service worker (`public/sw.js`)
// turns it into a fixed sentence.

import { fromBase64url } from "./identity";
import type { Phone, PushSubscriptionJson } from "./phone.svelte";

export type PushSupport =
  /** Push works here. */
  | "ready"
  /** iPhone and iPad: only once the app is on the Home Screen. */
  | "add_to_home_screen"
  | "unsupported";

export function pushSupport(env: {
  serviceWorker: boolean;
  pushManager: boolean;
  notification: boolean;
  standalone: boolean;
  appleMobile: boolean;
}): PushSupport {
  if (env.serviceWorker && env.pushManager && env.notification) return "ready";
  if (env.appleMobile && !env.standalone) return "add_to_home_screen";
  return "unsupported";
}

export function browserPushSupport(): PushSupport {
  return pushSupport({
    serviceWorker: "serviceWorker" in navigator,
    pushManager: "PushManager" in window,
    notification: "Notification" in window,
    standalone:
      matchMedia("(display-mode: standalone)").matches ||
      (navigator as Navigator & { standalone?: boolean }).standalone === true,
    appleMobile: /iPhone|iPad/.test(navigator.userAgent),
  });
}

async function registration(): Promise<ServiceWorkerRegistration> {
  await navigator.serviceWorker.register("/sw.js");
  return navigator.serviceWorker.ready;
}

/** Whether this phone has a push subscription now. */
export async function pushOn(): Promise<boolean> {
  if (browserPushSupport() !== "ready") return false;
  const reg = await registration();
  return (await reg.pushManager.getSubscription()) !== null;
}

/**
 * Asks for permission (this must run from a tap), subscribes with the
 * computer's key and tells the computer. Returns an error text, or null.
 */
export async function turnPushOn(phone: Phone): Promise<string | null> {
  const permission = await Notification.requestPermission();
  if (permission !== "granted") return "Notifications are turned off for this app in the phone's settings.";
  const reg = await registration();
  const { key } = await phone.phoneCall({ method: "phone.push_key" });
  const old = await reg.pushManager.getSubscription();
  await old?.unsubscribe();
  const subscription = await reg.pushManager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: fromBase64url(key),
  });
  await phone.phoneCall({
    method: "phone.push_subscribe",
    params: { subscription: subscription.toJSON() as PushSubscriptionJson },
  });
  return null;
}

export async function turnPushOff(phone: Phone): Promise<void> {
  await unsubscribeHere();
  await phone.phoneCall({ method: "phone.push_subscribe", params: { subscription: null } });
}

/** Ends this phone's subscription without telling the computer (it forgot this phone already). */
export async function unsubscribeHere(): Promise<void> {
  if (browserPushSupport() !== "ready") return;
  const reg = await navigator.serviceWorker.getRegistration();
  await (await reg?.pushManager.getSubscription())?.unsubscribe();
}
