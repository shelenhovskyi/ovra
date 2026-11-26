// The phone app's service worker: only notifications (docs/specs/companion.md,
// section 6). A push holds a kind and an opaque id, never what an agent does,
// so the text is fixed here. Nothing is cached: the app needs the computer.

const TEXT = {
  approval: "An agent needs your approval.",
  failed: "An agent stopped with an error.",
};

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("push", (event) => {
  let kind = "approval";
  try {
    const data = event.data?.json();
    if (data && typeof data.kind === "string" && Object.hasOwn(TEXT, data.kind)) kind = data.kind;
  } catch {
    // Not JSON: still say that something waits.
  }
  event.waitUntil(
    self.registration.showNotification("Ovra", {
      body: TEXT[kind],
      tag: kind,
      icon: "/phone-icon-256.png",
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  event.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((windows) => {
      const open = windows.find((w) => new URL(w.url).origin === self.location.origin);
      return open ? open.focus() : self.clients.openWindow("/#/");
    }),
  );
});
