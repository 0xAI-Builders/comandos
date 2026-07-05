// OpenCode -> ComandOS. Instalacion (la hace `cc-agents setup`):
//   cp adapters/opencode-comandos.js ~/.config/opencode/plugin/
// Reporta eventos al motor via POST /event de cc-dash (127.0.0.1:4777).
export const Comandos = async ({ directory }) => {
  const post = (body) =>
    fetch("http://127.0.0.1:4777/event", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agent: "opencode", cwd: directory, ...body }),
    }).catch(() => {});
  return {
    event: async ({ event }) => {
      const p = event?.properties || {};
      if (event.type === "session.idle") {
        post({ event: "done" });
      } else if (event.type === "permission.asked" || event.type === "permission.updated") {
        post({ event: "waiting",
               msg: p?.title || p?.permission?.title || p?.metadata?.title || "" });
      } else if (event.type === "session.error") {
        post({ event: "waiting", msg: String(p?.error?.name || "session.error") });
      } else if (event.type === "message.updated" && p?.info?.role === "user") {
        post({ event: "working" });
      }
    },
  };
};
