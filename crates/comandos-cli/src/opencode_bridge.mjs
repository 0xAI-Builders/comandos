// SDK and JSONL transport only. Rust owns event decisions and current state.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
export function bridge({ directory, client }, binary, { signal, timeout = 5000 } = {}) {
  const child = spawn(binary, ['hook', 'opencode', '--bridge'], { stdio: ['pipe', 'pipe', 'ignore'] });
  const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  let stopped = false;
  let cancel;
  const cancelled = new Promise((done) => { cancel = done; });
  const close = () => {
    if (stopped) return;
    stopped = true;
    cancel({ done: true });
    child.stdin.destroy();
    child.stdout.destroy();
    signal?.removeEventListener('abort', close);
  };
  child.on('error', close);
  child.on('close', close);
  child.stdin.on('error', close);
  signal?.addEventListener('abort', close, { once: true });
  if (signal?.aborted) close();
  child.unref();
  child.stdin.unref();
  child.stdout.unref();
  let queue = Promise.resolve();
  const next = async () => {
    let timer;
    try {
      return await Promise.race([
        lines.next(), cancelled,
        new Promise((_, fail) => { timer = setTimeout(() => fail(new Error('native transport timeout')), timeout); }),
      ]);
    } finally { clearTimeout(timer); }
  };
  return { event: ({ event }) => {
    queue = queue.then(async () => {
      if (stopped) return;
      child.stdin.ref();
      child.stdout.ref();
      child.stdin.write(JSON.stringify({ directory, event }) + '\n');
      const frame = await next();
      if (frame.done) { close(); return; }
      const request = JSON.parse(frame.value);
      if (request !== null) {
        let timer;
        let response;
        try {
          response = await Promise.race([
            Promise.resolve().then(() => client.session.get(request)).catch(() => null), cancelled,
            new Promise((done) => { timer = setTimeout(() => done(null), timeout); }),
          ]);
        } finally { clearTimeout(timer); }
        if (stopped) return;
        child.stdin.write(JSON.stringify(response ?? null) + '\n');
      }
      const ack = await next();
      if (ack.done) close();
      else if (JSON.parse(ack.value) !== null) throw new Error('invalid native ack');
    }).catch(close).finally(() => {
      child.stdin.unref();
      child.stdout.unref();
    });
    return queue;
  } };
}
