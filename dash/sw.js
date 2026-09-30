// Service worker minimo: habilita instalar como app (PWA) y una pantalla
// offline decente. NO cachea /state ni las APIs (siempre en vivo).
const SHELL = "comandos-shell-v13";
self.addEventListener("install", (e) => {
  self.skipWaiting();
  e.waitUntil(caches.open(SHELL).then((c) => c.addAll(["/", "/manifest.webmanifest"])));
});
self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== SHELL).map((k) => caches.delete(k))))
  );
  self.clients.claim();
});
self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  // APIs y acciones: SIEMPRE red, nunca cache (datos vivos, comandos reales).
  const live = [
    "/state", "/events", "/conf", "/prefs", "/ssh",
    "/tabs", "/tab-history", "/remote-state", "/remote-qr.png", "/webterm-token", "/term",
    "/operator", // Includes /operator/chat/stream and durable action results.
    "/session-", "/extension-usage", "/model/", "/usage/", "/providers",
    "/news/", "/push/"
  ];
  if (url.origin !== self.location.origin) return;
  if (e.request.method !== "GET" || live.some((p) => url.pathname.startsWith(p))) return;
  // Cache only the static shell/assets, never an unknown/new API response.
  if (url.pathname !== "/" && !/\.(?:html|css|js|png|svg|ico|woff2?|ttf|webmanifest)$/.test(url.pathname)) return;
  // La URL remota usa token en querystring: red primero sin guardar esa variante.
  if (url.pathname === "/" && url.searchParams.has("token")) {
    e.respondWith(fetch(e.request, {cache: "no-cache"}).catch(() => caches.match("/")));
    return;
  }
  // El shell: red primero y SIEMPRE revalidado (el caché HTTP del teléfono
  // servía un index.html viejo); el caché propio queda solo para offline.
  e.respondWith(
    fetch(e.request, {cache: "no-cache"})
      .then((r) => { const cp = r.clone(); caches.open(SHELL).then((c) => c.put(e.request, cp)); return r; })
      .catch(() => caches.match(e.request).then((m) => m || caches.match("/")))
  );
});

// ---- Web Push (N4). La notificación se muestra solo con el payload recibido
// (no consulta el host: Tailscale puede no estar accesible). D6: proyecto +
// título breve; el extracto se ve al abrir la app autenticada.
function eventIdFrom(value) {
  const id = typeof value === "string" ? value : "";
  return id.length <= 128 && !/[\u0000-\u001f]/.test(id) ? id : "";
}
self.addEventListener("push", (e) => {
  let data = {};
  try { data = e.data ? e.data.json() : {}; } catch (err) { data = {}; }
  if (!data || typeof data !== "object") data = {};
  const id = eventIdFrom(data.eventId);
  const title = String(data.title || "CommandOS").slice(0, 60);
  const body = String(data.body || "").slice(0, 120);
  // Tag estable por evento: un reintento reemplaza al aviso anterior.
  const tag = id ? "comandos-event-" + id : (data.tag === "comandos-test" ? "comandos-test" : "comandos");
  e.waitUntil(self.registration.showNotification(title, {
    body, tag, renotify: false, icon: "/icon-192.png", badge: "/icon-192.png", data: { eventId: id }
  }));
});
self.addEventListener("notificationclick", (e) => {
  e.notification.close();
  const id = eventIdFrom(e.notification.data && e.notification.data.eventId);
  // Nunca se usa una URL del payload ni se ponen tokens en la URL.
  const target = new URL(id ? "/?event=" + encodeURIComponent(id) : "/", self.location.origin).href;
  e.waitUntil((async () => {
    const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    for (const w of wins) {
      let origin = "";
      try { origin = new URL(w.url).origin; } catch (err) { continue; }
      if (origin !== self.location.origin) continue;
      // Ventana abierta: enfocarla sin recargar (conserva terminal y borrador).
      if (typeof w.focus === "function") await w.focus();
      w.postMessage({ type: "comandos:open-event", eventId: id });
      return;
    }
    if (self.clients.openWindow) await self.clients.openWindow(target);
  })());
});
