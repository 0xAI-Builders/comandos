(function(root){
  "use strict";
  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  const lerp = (a, b, t) => a + (b - a) * t;
  const smoothstep = t => { t = clamp(t, 0, 1); return t * t * (3 - 2 * t); };

  function hashSeed(value){
    let h = 0x811c9dc5;
    for(const ch of String(value)){
      h ^= ch.charCodeAt(0);
      h = Math.imul(h, 0x01000193);
    }
    h >>>= 0;
    return h || 0x9e3779b9;
  }

  function createRng(seed){
    let state = (seed >>> 0) || 0x9e3779b9;
    const children = new Map();
    const api = {
      next(){
        state ^= state << 13;
        state ^= state >>> 17;
        state ^= state << 5;
        return (state >>> 0) / 4294967296;
      },
      range(lo, hi){ return lerp(lo, hi, api.next()); },
      int(lo, hi){ return lo + Math.floor(api.next() * (hi - lo + 1)); },
      pick(items){ return items[api.int(0, items.length - 1)]; },
      fork(label){
        if(!children.has(label)){
          children.set(label, createRng(hashSeed(`${state}:${label}`)));
        }
        return children.get(label);
      },
      get state(){ return state >>> 0; },
    };
    return api;
  }

  function createSpring(value, stiffness, damping){
    return {value, velocity:0, stiffness, damping};
  }

  function stepSpring(spring, target, dt){
    dt = clamp(dt, 0, 1 / 20);
    spring.velocity += ((target - spring.value) * spring.stiffness -
                        spring.velocity * spring.damping) * dt;
    spring.value += spring.velocity * dt;
    if(!Number.isFinite(spring.value) || !Number.isFinite(spring.velocity)){
      spring.value = target;
      spring.velocity = 0;
    }
    return spring.value;
  }

  function createRingStats(size){
    size = Math.max(1, Math.floor(size) || 1);
    const values = new Float64Array(size);
    let cursor = 0;
    let count = 0;
    let total = 0;
    return {
      push(value){
        const old = count === size ? values[cursor] : 0;
        values[cursor] = value;
        cursor = (cursor + 1) % size;
        if(count < size) count += 1;
        total += value - old;
      },
      average(){ return count ? total / count : 0; },
      percentile(fraction){
        if(!count) return 0;
        const sorted = new Float64Array(count);
        for(let i = 0; i < count; i += 1) sorted[i] = values[i];
        sorted.sort();
        const rank = Math.max(0, Math.min(count - 1,
          Math.ceil(clamp(fraction, 0, 1) * count) - 1));
        return sorted[rank];
      },
      get count(){ return count; },
      get size(){ return size; },
    };
  }

  function createDiagnostics(clock){
    let now;
    if(typeof clock === "function") now = clock;
    else if(clock && typeof clock.now === "function") now = () => clock.now();
    else now = () => root.performance.now();

    const stats = {
      update: createRingStats(120),
      render: createRingStats(120),
    };
    const counters = {update: 0, render: 0};

    function begin(){ return now(); }
    function end(kind, start){
      const duration = now() - start;
      const series = stats[kind];
      if(series){
        series.push(duration);
        counters[kind] += 1;
      }
      return duration;
    }
    function snapshot(){
      const update = {
        count: stats.update.count,
        average: stats.update.average(),
        p95: stats.update.percentile(0.95),
      };
      const render = {
        count: stats.render.count,
        average: stats.render.average(),
        p95: stats.render.percentile(0.95),
      };
      return Object.freeze({
        update: Object.freeze(update),
        render: Object.freeze(render),
        updateAverage: update.average,
        updateP95: update.p95,
        renderAverage: render.average,
        renderP95: render.p95,
      });
    }

    return {begin, end, counters, snapshot};
  }

  NS.Core = Object.freeze({clamp, lerp, smoothstep, hashSeed, createRng,
    createSpring, stepSpring, createRingStats, createDiagnostics});
})(typeof window !== "undefined" ? window : globalThis);
