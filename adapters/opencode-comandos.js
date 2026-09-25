// OpenCode -> ComandOS. Only process/session/configuration metadata is persisted.
import { mkdir, readFile, writeFile, rename } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const Comandos = async ({ directory, client }) => {
  const root = join(homedir(), '.claude/hooks/native-processes');
  let start = '';
  try { start = (await readFile(`/proc/${process.pid}/stat`, 'utf8')).split(')').slice(1).join(')').trim().split(/\s+/)[19]; } catch {}
  let current = null;
  const persist = async (sessionID, patch = {}) => {
    if (!start || !/^[A-Za-z0-9_-]{1,256}$/.test(sessionID || '')) return;
    try {
      const response = await client.session.get({ path: { id: sessionID } });
      const session = response?.data || response;
      if (!session || session.id !== sessionID || session.parentID) return;
      current = { ...(current?.sessionId === sessionID ? current : {}),
        pid: process.pid, start, harness: 'opencode', sessionId: sessionID,
        parentId: '', updatedAt: Date.now(), ...patch };
      await mkdir(root, { recursive: true, mode: 0o700 });
      const path = join(root, `${process.pid}.json`);
      const temp = `${path}.${Math.random().toString(16).slice(2)}.tmp`;
      await writeFile(temp, JSON.stringify(current), { mode: 0o600, flag: 'wx' });
      await rename(temp, path);
    } catch {}
  };
  const post = (event) => fetch('http://127.0.0.1:4777/event', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ agent: 'opencode', cwd: directory, event }),
  }).catch(() => {});
  return {
    event: async ({ event }) => {
      const p = event?.properties || {};
      const sessionID = p.sessionID || p.info?.sessionID;
      if (event.type === 'session.idle') {
        await persist(sessionID, { busy: false }); post('done');
      } else if (event.type === 'permission.asked' || event.type === 'permission.updated' || event.type === 'session.error') {
        await persist(sessionID, { busy: true }); post('waiting');
      } else if (event.type === 'message.updated') {
        const info = p.info || {};
        const model = info.modelID && info.providerID ? `${info.providerID}/${info.modelID}` : '';
        const patch = model ? { model } : {};
        if (info.role === 'user') { patch.busy = true; post('working'); }
        await persist(sessionID, patch);
      }
    },
  };
};
