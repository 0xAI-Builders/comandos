// OpenCode -> ComandOS. Shim mínimo: la API de plugins exige JS; todo lo demás vive
// en `comandos hook opencode`. Aquí solo se filtran los eventos que importan (no se
// lanza un proceso por cada fragmento de mensaje) y se consulta la sesión con el
// SDK, que solo existe dentro del plugin.
import { spawn } from 'node:child_process';

const EVENTS = new Set(['session.idle', 'session.error', 'permission.asked', 'permission.updated', 'message.updated']);

export const Comandos = async ({ directory, client }) => process.env.COMANDOS_SILENT_AGENT === '1' ? {} : {
  event: async ({ event }) => {
    if (!EVENTS.has(event?.type)) return;
    const id = event.properties?.sessionID || event.properties?.info?.sessionID;
    const s = /^[A-Za-z0-9_-]{1,256}$/.test(id || '')
      ? await client.session.get({ path: { id } }).then((r) => r?.data || r, () => null) : null;
    const input = JSON.stringify({ directory, event, session: s && { id: s.id, parentID: s.parentID } });
    const child = spawn('comandos', ['hook', 'opencode'], { stdio: ['pipe', 'ignore', 'ignore'] });
    await new Promise((done) => {
      child.on('error', done);
      child.on('close', done);
      child.stdin.on('error', () => {});
      child.stdin.end(input);
    });
  },
};
