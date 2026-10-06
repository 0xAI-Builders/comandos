// Avisos push en este dispositivo (N4). El permiso SOLO se pide tras un gesto
// explícito (botón en Ajustes > Notificaciones); una denegación no se vuelve a
// pedir en bucle. También enruta ?event=<id> y los mensajes del service worker
// a un CustomEvent "comandos:open-event" que consume el centro de avisos (N2).
(function (root, factory) {
  if (typeof module === "object" && module.exports) module.exports = factory();
  else root.PushSettings = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  const ENABLED_KEY = "comandos.push.enabled";
  const DEVICE_KEY = "comandos.deviceId";

  function urlBase64ToUint8Array(value) {
    const padded = String(value).replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - value.length % 4) % 4);
    const raw = atob(padded);
    const out = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
    return out;
  }

  function support(env) {
    if (!env.isSecureContext) return { ok: false, reason: "Los avisos push necesitan abrir CommandOS por HTTPS (la URL de Tailscale)." };
    if (!env.navigator || !env.navigator.serviceWorker) return { ok: false, reason: "Este navegador no tiene service worker." };
    if (!env.PushManager) return { ok: false, reason: "Este navegador no soporta avisos push." };
    if (!env.Notification) return { ok: false, reason: "Este navegador no permite notificaciones." };
    return { ok: true, reason: "" };
  }

  const DENIED_HELP = "El permiso de notificaciones está bloqueado para este sitio. Actívalo en los ajustes del navegador (candado de la barra de direcciones > Notificaciones) y vuelve a pulsar Activar.";

  function createController(env) {
    const store = env.storage;
    const get = k => { try { return store.getItem(k); } catch (e) { return null; } };
    const set = (k, v) => { try { if (v == null) store.removeItem(k); else store.setItem(k, v); } catch (e) { /* private */ } };

    function deviceId() {
      let id = get(DEVICE_KEY);
      if (!id) {
        id = (env.crypto && env.crypto.randomUUID) ? env.crypto.randomUUID() : "dev-" + Date.now().toString(36) + Math.random().toString(36).slice(2);
        set(DEVICE_KEY, id);
      }
      return id;
    }

    async function registration() {
      return env.navigator.serviceWorker.ready;
    }

    async function current() {
      const s = support(env);
      if (!s.ok) return { state: "unsupported", message: s.reason };
      if (env.Notification.permission === "denied") return { state: "denied", message: DENIED_HELP };
      const reg = await registration();
      const sub = await reg.pushManager.getSubscription();
      if (sub && env.Notification.permission === "granted") return { state: "enabled", subscription: sub, message: "Este dispositivo recibe avisos cuando CommandOS no está visible." };
      return { state: "disabled", message: "Recibe un aviso breve (proyecto y título) cuando ningún CommandOS esté visible." };
    }

    // Must be called from a click handler: the permission prompt needs a gesture.
    async function enable() {
      const s = support(env);
      if (!s.ok) return { state: "unsupported", message: s.reason };
      let permission = env.Notification.permission;
      if (permission === "denied") return { state: "denied", message: DENIED_HELP };
      if (permission !== "granted") permission = await env.Notification.requestPermission();
      if (permission !== "granted") {
        set(ENABLED_KEY, null);
        return { state: permission === "denied" ? "denied" : "disabled",
          message: permission === "denied" ? DENIED_HELP : "No se concedió el permiso. Puedes intentarlo de nuevo cuando quieras." };
      }
      const key = await env.fetchJson("GET", "/push/key");
      if (!key || !key.available || !key.publicKey) return { state: "error", message: (key && key.error) || "El servidor no tiene push disponible." };
      const reg = await registration();
      let sub = await reg.pushManager.getSubscription();
      if (!sub) sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: urlBase64ToUint8Array(key.publicKey) });
      await env.fetchJson("POST", "/push/subscription", { subscription: sub.toJSON(), deviceId: deviceId(),
        userAgent: String(env.navigator.userAgent || "").slice(0, 200) });
      set(ENABLED_KEY, "1");
      return { state: "enabled", subscription: sub, message: "Listo. Pulsa «Enviar aviso de prueba» con la app en segundo plano o la pantalla bloqueada." };
    }

    async function disable() {
      const reg = await registration();
      const sub = await reg.pushManager.getSubscription();
      set(ENABLED_KEY, null);
      if (sub) {
        await env.fetchJson("DELETE", "/push/subscription", { endpoint: sub.endpoint }).catch(() => {});
        await sub.unsubscribe().catch(() => {});
      }
      return { state: "disabled", message: "Avisos push desactivados en este dispositivo." };
    }

    async function test() {
      const reg = await registration();
      const sub = await reg.pushManager.getSubscription();
      if (!sub) return { state: "disabled", message: "Primero activa los avisos en este dispositivo." };
      const r = await env.fetchJson("POST", "/push/test", { endpoint: sub.endpoint });
      if (r && r.removed) { set(ENABLED_KEY, null); return { state: "disabled", message: "El servicio push dio la suscripción por caducada; actívala de nuevo." }; }
      return { state: "enabled", message: r && r.ok ? "Aviso de prueba enviado al servicio push. Si no llega, revisa el modo No molestar y los ajustes de Chrome." : `El servicio push respondió ${r && r.status}.` };
    }

    // Keeps the server in sync when the browser rotated the subscription.
    // Never prompts: only runs after an earlier explicit enable with permission.
    async function resync() {
      if (get(ENABLED_KEY) !== "1" || !support(env).ok || env.Notification.permission !== "granted") return false;
      const reg = await registration();
      const sub = await reg.pushManager.getSubscription();
      if (!sub) { set(ENABLED_KEY, null); return false; }
      await env.fetchJson("POST", "/push/subscription", { subscription: sub.toJSON(), deviceId: deviceId() });
      return true;
    }

    return { current, enable, disable, test, resync, deviceId };
  }

  function browserEnv() {
    const token = () => { try { return localStorage.getItem("cc_token") || ""; } catch (e) { return ""; } };
    return {
      navigator, Notification: typeof Notification !== "undefined" ? Notification : null,
      PushManager: typeof PushManager !== "undefined" ? PushManager : null,
      isSecureContext: typeof isSecureContext !== "undefined" ? isSecureContext : false,
      storage: localStorage, crypto: typeof crypto !== "undefined" ? crypto : null,
      async fetchJson(method, url, body) {
        const headers = {};
        const t = token();
        if (t) headers["X-Comandos-Token"] = t;
        if (body) headers["Content-Type"] = "application/json";
        const r = await fetch(url, { method, headers, body: body ? JSON.stringify(body) : undefined });
        const j = await r.json().catch(() => ({}));
        if (!r.ok && r.status !== 503) throw new Error(j.error || r.statusText || "No se completó");
        return j;
      },
    };
  }

  function routeEvent(win, id) {
    if (!id) return;
    win.dispatchEvent(new win.CustomEvent("comandos:open-event", { detail: { eventId: id } }));
  }

  function installEventRouting(win) {
    const params = new URLSearchParams(win.location.search);
    const id = params.get("event");
    if (id !== null) {
      params.delete("event");
      const qs = params.toString();
      win.history.replaceState(win.history.state, "", win.location.pathname + (qs ? "?" + qs : "") + win.location.hash);
      // Let the notification center register its listener first.
      win.setTimeout(() => routeEvent(win, id.slice(0, 128)), 0);
    }
    if (win.navigator.serviceWorker) {
      win.navigator.serviceWorker.addEventListener("message", e => {
        const m = e.data || {};
        if (m.type === "comandos:open-event") routeEvent(win, String(m.eventId || "").slice(0, 128));
      });
    }
  }

  function installUi(doc, controller) {
    const box = doc.getElementById("push-settings");
    if (!box) return;
    const status = box.querySelector("#push-status");
    const btnOn = box.querySelector("#push-enable"), btnOff = box.querySelector("#push-disable"), btnTest = box.querySelector("#push-test");
    const show = r => {
      status.textContent = r.message || "";
      const enabled = r.state === "enabled";
      btnOn.hidden = enabled || r.state === "unsupported";
      btnOff.hidden = !enabled;
      btnTest.hidden = !enabled;
      box.dataset.state = r.state;
    };
    const run = fn => async () => {
      [btnOn, btnOff, btnTest].forEach(b => { b.disabled = true; });
      try { show(await fn()); }
      catch (err) { status.textContent = "No se pudo completar: " + (err.message || err); }
      finally { [btnOn, btnOff, btnTest].forEach(b => { b.disabled = false; }); }
    };
    btnOn.addEventListener("click", run(controller.enable));
    btnOff.addEventListener("click", run(controller.disable));
    btnTest.addEventListener("click", run(controller.test));
    controller.current().then(show).catch(() => {});
  }

  function install() {
    installEventRouting(window);
    let controller = null;
    try { controller = createController(browserEnv()); } catch (e) { return; }
    installUi(document, controller);
    controller.resync().catch(() => {});
  }

  const api = { createController, support, urlBase64ToUint8Array, installEventRouting, DENIED_HELP };
  if (typeof document !== "undefined" && document.currentScript && document.currentScript.hasAttribute("data-autoinstall")) {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", install); else install();
  }
  return api;
});
