// The panel's service worker: Web Push alone, nothing is cached.
// The payload is sa_server's outlets': {title, body, url, tag, level}.

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));

self.addEventListener("push", (e) => {
  const { title, body, url, tag, level } = e.data.json();
  e.waitUntil(
    self.registration.showNotification(title, {
      body,
      tag,
      renotify: true,
      icon: "/icon-192.png",
      data: { url },
      requireInteraction: level === "high",
      silent: level === "low",
    }),
  );
});

self.addEventListener("notificationclick", (e) => {
  e.notification.close();
  const to = new URL(e.notification.data.url, self.location.origin);
  if (to.origin !== self.location.origin) return;
  e.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then(async (open) => {
      const shown = open.find((c) => c.visibilityState === "visible") ?? open[0];
      if (shown === undefined) return self.clients.openWindow(to.href);
      await shown.focus();
      // the page moves its own location store: a navigation would reload the panel
      shown.postMessage({ type: "sa:open", path: to.pathname + to.search });
    }),
  );
});
