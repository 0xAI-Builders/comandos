// OpenCode -> ComandOS. Shim mínimo: la API de plugins exige JS; todo lo demás vive
// en `comandos hook opencode`. Aquí solo se filtran los eventos que importan (no se
// lanza un proceso por cada fragmento de mensaje), se consulta la sesión con el SDK
// (solo existe dentro del plugin) y se guarda en memoria el registro del proceso,
// como el plugin original: `comandos` lo recibe, lo fusiona y devuelve el nuevo.
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

const EVENTS = new Set(['session.idle', 'session.error', 'permission.asked', 'permission.updated', 'message.updated']);
// El `comandos` que instala ComandOS primero; si no está, el del PATH.
const bin = () => {
  const local = join(process.env.HOME || homedir(), '.local/share/comandos/bin/comandos');
  return existsSync(local) ? local : 'comandos';
};

export const Comandos = async ({ directory, client }) => {
  if (process.env.COMANDOS_SILENT_AGENT === '1') return {};
  let current = null;
  return {
    event: async ({ event }) => {
      if (!EVENTS.has(event?.type)) return;
      const id = event.properties?.sessionID || event.properties?.info?.sessionID;
      const s = /^[A-Za-z0-9_-]{1,256}$/.test(id || '')
        ? await client.session.get({ path: { id } }).then((r) => r?.data || r, () => null) : null;
      const input = JSON.stringify({ directory, event, session: s && { id: s.id, parentID: s.parentID }, current });
      // Nunca más de 5 s: un hook colgado no puede detener a OpenCode.
      const child = spawn(bin(), ['hook', 'opencode'], { stdio: ['pipe', 'pipe', 'ignore'], timeout: 5000 });
      let out = '';
      child.stdout?.on('data', (chunk) => { out += chunk; });
      await new Promise((done) => {
        child.on('error', done);
        child.on('close', done);
        child.stdin.on('error', () => {});
        child.stdin.end(input);
      });
      try {
        const next = JSON.parse(out);
        if (next && typeof next === 'object') current = next;
      } catch {}
    },
  };
};
