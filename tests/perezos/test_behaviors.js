"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

global.window = global;
require("../../dash/perezos/core.js");
const behaviorPath = path.resolve(__dirname, "../../dash/perezos/behaviors.js");
if(fs.existsSync(behaviorPath)) require(behaviorPath);

const C = global.ComandOSPerezOS.Core;
const B = global.ComandOSPerezOS.Behaviors;

const IDS = Object.freeze([
  "perceive", "orient-gaze", "refocus", "blink", "turn-head", "breathe",
  "brace", "shift-weight", "reach", "search", "open-grip", "release", "swing",
  "touch", "close-grip", "pull", "settle", "stretch", "scratch", "groom",
  "yawn", "doze", "wake", "inspect", "point", "recoil", "celebrate",
  "comfort-cable", "slip", "recover", "neutral",
]);

const DRIVE_NAMES = Object.freeze([
  "sleepiness", "curiosity", "attention", "gripConfidence", "fatigue", "comfort",
  "boredom", "satisfaction", "alertness", "habituation",
]);

function baseContext(status = "idle", overrides = {}){
  return {
    sessionId:"session-test",
    status,
    role:"daily",
    costume:"",
    contextPressure:"low",
    theme:"noche",
    expanded:false,
    ...overrides,
  };
}

function collect(seed, count, context = baseContext()){
  const director = B.createDirector(seed);
  B.updateContext(director, context, 0);
  const performances = [];
  for(let index = 0; index < count; index += 1){
    const nowMs = index * 61000;
    const performance = B.nextPerformance(director, nowMs);
    performances.push(performance);
    assert.equal(B.completePerformance(director, performance,
      nowMs + performance.durationMs), true);
  }
  return {director, performances};
}

test("behavior namespace is installed", () => {
  assert.ok(B, "ComandOSPerezOS.Behaviors must be defined");
});

test("primitive registry is exact, complete, and deeply immutable", () => {
  assert.deepEqual(Object.keys(B.PRIMITIVES), IDS);
  assert.equal(Object.isFrozen(B.PRIMITIVES), true);
  for(const id of IDS){
    const primitive = B.PRIMITIVES[id];
    assert.deepEqual(Object.keys(primitive), [
      "channels", "duration", "interruptible", "safeEnd", "precondition", "targets",
    ], id);
    assert.equal(Object.isFrozen(primitive), true, `${id} metadata`);
    assert.equal(Object.isFrozen(primitive.channels), true, `${id} channels`);
    assert.equal(Object.isFrozen(primitive.duration), true, `${id} duration`);
    assert.ok(primitive.channels.length > 0, `${id} channels`);
    assert.ok(Number.isFinite(primitive.duration.minMs) && primitive.duration.minMs > 0,
      `${id} min duration`);
    assert.ok(primitive.duration.maxMs >= primitive.duration.minMs, `${id} duration order`);
    assert.equal(typeof primitive.interruptible, "boolean", `${id} interruptible`);
    assert.ok(primitive.safeEnd > 0 && primitive.safeEnd <= 1, `${id} safe cut`);
    assert.equal(typeof primitive.precondition, "function", `${id} precondition`);
    assert.equal(typeof primitive.targets, "function", `${id} targets`);
    const targets = primitive.targets({side:"left", target:"notice", distance:0.5,
      grip:0.5, intensity:0.5, headLead:0.5, furFollowThrough:0.5});
    assert.equal(Object.isFrozen(targets), true, `${id} target result`);
    for(const [channel, value] of Object.entries(targets)){
      assert.ok(primitive.channels.includes(channel), `${id} undeclared ${channel}`);
      assert.ok(Number.isFinite(value), `${id} finite ${channel}`);
    }
  }
});

test("body-lean primitives preserve side and effort variation inside the safe envelope", () => {
  const cases = [
    ["brace", "intensity"],
    ["shift-weight", "distance"],
    ["pull", "intensity"],
    ["recoil", "intensity"],
    ["recover", "intensity"],
  ];
  const highMagnitudes = new Set();
  for(const [name, varying] of cases){
    const primitive = B.PRIMITIVES[name];
    const sample = (side, amount) => primitive.targets({side, intensity:0.5,
      distance:0.5, grip:0.6, [varying]:amount})["body-lean-x"];
    const leftLow = sample("left", 0.2);
    const leftHigh = sample("left", 0.8);
    const rightLow = sample("right", 0.2);
    const rightHigh = sample("right", 0.8);
    for(const target of [leftLow, leftHigh, rightLow, rightHigh]){
      assert.ok(Math.abs(target) < 2, `${name} ${target} saturates the ±2 solver envelope`);
    }
    assert.notEqual(leftLow, leftHigh, `${name} loses ${varying} variation after clamping`);
    assert.notEqual(rightLow, rightHigh, `${name} loses ${varying} variation after clamping`);
    assert.equal(Math.sign(leftLow), -Math.sign(rightLow), `${name} loses side direction`);
    assert.equal(Math.sign(leftHigh), -Math.sign(rightHigh), `${name} loses side direction`);
    highMagnitudes.add(Math.abs(leftHigh).toFixed(4));
  }
  assert.equal(highMagnitudes.size, cases.length,
    "brace/shift/pull/recoil/recover require distinct authored response curves");
});

test("personality is stable, varied, frozen, and created from a separate fork", () => {
  const first = B.createDirector("personality-a");
  const again = B.createDirector("personality-a");
  assert.deepEqual(first.personality, again.personality);
  assert.equal(Object.isFrozen(first.personality), true);
  assert.deepEqual(Object.keys(first.personality), [
    "preferredSide", "blinkMs", "curiosity", "sleepBias", "gripCaution",
  ]);
  const variants = new Set(Array.from({length:32}, (_, seed) =>
    JSON.stringify(B.createDirector(`personality-${seed}`).personality)));
  assert.ok(variants.size >= 28, `${variants.size} personalities`);

  const source = fs.readFileSync(behaviorPath, "utf8");
  let rootNextCalls = 0;
  const sandboxCore = {...C, createRng(seed){
    const rootRng = C.createRng(seed);
    return {
      next(){ rootNextCalls += 1; return rootRng.next(); },
      range(lo, hi){ rootNextCalls += 1; return rootRng.range(lo, hi); },
      int(lo, hi){ rootNextCalls += 1; return rootRng.int(lo, hi); },
      pick(items){ rootNextCalls += 1; return rootRng.pick(items); },
      fork(label){ return rootRng.fork(label); },
      get state(){ return rootRng.state; },
    };
  }};
  const sandbox = {ComandOSPerezOS:{Core:sandboxCore}};
  sandbox.globalThis = sandbox;
  vm.runInNewContext(source, sandbox);
  sandbox.ComandOSPerezOS.Behaviors.createDirector("stream-check");
  assert.equal(rootNextCalls, 0, "personality and actions must both use child streams");
});

test("same seed and inputs produce identical immutable performances", () => {
  const a = collect("repeatable", 48).performances;
  const b = collect("repeatable", 48).performances;
  assert.deepEqual(a, b);
  assert.deepEqual(a.map(B.performanceSignature), b.map(B.performanceSignature));
  for(const performance of a){
    assert.equal(Object.isFrozen(performance), true);
    assert.equal(Object.isFrozen(performance.phases), true);
    assert.ok(performance.durationMs > 0);
    assert.equal(performance.durationMs,
      performance.phases.reduce((sum, phase) => sum + phase.durationMs, 0));
    for(const phase of performance.phases){
      assert.equal(Object.isFrozen(phase), true);
      assert.equal(Object.isFrozen(phase.params), true);
      assert.equal(Object.isFrozen(phase.targets), true);
      assert.ok(phase.durationMs > 0);
      assert.ok(phase.safeEnd > 0 && phase.safeEnd <= phase.durationMs);
      assert.equal(phase.endMs, phase.startMs + phase.durationMs);
    }
  }
});

test("idle selection rejects every family in the completed last-eight ring", () => {
  const director = B.createDirector("last-eight-selection");
  B.updateContext(director, baseContext("idle"), 0);
  const recent = [];
  for(let index = 0; index < 96; index += 1){
    const nowMs = index * 61000;
    const performance = B.nextPerformance(director, nowMs);
    assert.equal(recent.includes(performance.family), false,
      `${performance.family} recurred with ${recent.join(",")} still recent`);
    assert.equal(B.completePerformance(director, performance,
      nowMs + performance.durationMs), true);
    recent.push(performance.family);
    if(recent.length > 8) recent.shift();
  }
});

test("two-family waiting grammar relaxes oldest recent family deterministically", () => {
  const director = B.createDirector("waiting-relaxation");
  B.updateContext(director, baseContext("waiting"), 0);
  const families = [];
  for(let index = 0; index < 6; index += 1){
    const nowMs = index * 10000;
    const performance = B.nextPerformance(director, nowMs);
    families.push(performance.family);
    B.completePerformance(director, performance, nowMs + performance.durationMs);
  }
  assert.equal(new Set(families).size, 2);
  assert.deepEqual(families.slice(2), families.slice(0, 4));
});

test("generated action durations stay inside primitive metadata ranges", () => {
  for(let seed = 0; seed < 128; seed += 1){
    const {performances} = collect(`duration-bounds-${seed}`, 24);
    for(const performance of performances){
      for(const phase of performance.phases){
        const duration = B.PRIMITIVES[phase.primitive].duration;
        assert.ok(phase.actionMs >= duration.minMs,
          `${phase.primitive} action ${phase.actionMs} < ${duration.minMs}`);
        assert.ok(phase.actionMs <= duration.maxMs,
          `${phase.primitive} action ${phase.actionMs} > ${duration.maxMs}`);
      }
    }
  }
});

test("context normalization and visible-time clock handle hostile time values", () => {
  const director = B.createDirector("context");
  const normalized = B.updateContext(director, {
    sessionId:42,
    status:" WAITING ",
    role:" DAILY ",
    costume:null,
    contextPressure:"LOUD",
    theme:"  NOCHE ",
    expanded:1,
    supports:{
      "front-left":{mode:"loaded", load:4},
      "front-right":"released",
      malicious:{mode:"loaded"},
    },
  }, 1000);
  assert.deepEqual(normalized, {
    sessionId:"42", status:"waiting", role:"daily", costume:"",
    contextPressure:"low", theme:"noche", expanded:true,
    supports:{
      "front-left":{mode:"loaded", load:1},
      "front-right":{mode:"released", load:0},
      "rear-left":{mode:"released", load:0},
      "rear-right":{mode:"released", load:0},
    },
  });
  assert.equal(Object.isFrozen(normalized), true);
  assert.equal(Object.isFrozen(normalized.supports), true);
  assert.equal(director.visibleTimeMs, 1000);
  B.updateContext(director, baseContext("idle"), 100);
  assert.equal(director.visibleTimeMs, 1000, "visible time cannot rewind");
  B.updateContext(director, baseContext("idle"), Number.POSITIVE_INFINITY);
  assert.equal(director.visibleTimeMs, 1000, "non-finite time is ignored");
  B.updateContext(director, baseContext("idle"), 1e300);
  assert.equal(director.visibleTimeMs, Number.MAX_SAFE_INTEGER);
  assert.deepEqual(Object.keys(director.drives), DRIVE_NAMES);
  for(const value of Object.values(director.drives)){
    assert.ok(Number.isFinite(value) && value >= 0 && value <= 1);
  }
});

test("all state templates have exact priorities, deadlines, and narrative actions", () => {
  const cases = [
    ["idle", 10, null, []],
    ["working", 30, null, ["inspect"]],
    ["done", 40, null, ["settle"]],
    ["waiting", 90, 5000, ["point"]],
    ["dead", 100, 8000, ["perceive", "close-grip"]],
  ];
  for(const [status, priority, deadlineMs, primitives] of cases){
    const director = B.createDirector(`state-${status}`);
    B.updateContext(director, baseContext(status, {supports:{
      "front-left":{mode:"loaded", load:1},
      "front-right":{mode:"released", load:0},
    }}), 0);
    const performance = B.nextPerformance(director, 0);
    assert.equal(performance.state, status);
    assert.equal(performance.priority, priority);
    assert.equal(performance.deadlineMs, deadlineMs);
    const ids = performance.phases.map(phase => phase.primitive);
    for(const primitive of primitives) assert.ok(ids.includes(primitive), `${status}: ${primitive}`);
    if(status === "working") assert.equal(performance.target, "command-packet");
    if(status === "waiting"){
      assert.equal(performance.target, "notice");
      assert.equal(performance.side, "right", "point must use the free front claw");
      const point = performance.phases.find(phase => phase.primitive === "point");
      const curls = Object.entries(point.targets).filter(([channel]) => channel.endsWith("-curl"))
        .map(([, value]) => value);
      assert.equal(curls.filter(value => value < 0.2).length, 1,
        "one pointing digit must remain extended");
      assert.equal(curls.filter(value => value > 0.5).length, 2,
        "the other two digits must curl out of the way");
    }
    if(status === "dead") assert.equal(performance.target, "signal");
  }
});

test("every working variation advances carefully before inspecting the command packet", () => {
  for(let seed = 0; seed < 64; seed += 1){
    const director = B.createDirector(`working-variation-${seed}`);
    B.updateContext(director, baseContext("working"), 0);
    const performance = B.nextPerformance(director, 0);
    const ids = performance.phases.map(phase => phase.primitive);
    assert.ok(ids.includes("shift-weight"), `${performance.family} does not advance`);
    assert.ok(ids.includes("inspect"), `${performance.family} does not inspect`);
    assert.equal(performance.target, "command-packet");
  }
});

test("waiting and dead immediately preempt lower-priority active work", () => {
  const director = B.createDirector("preemption");
  B.updateContext(director, baseContext("idle"), 0);
  const idle = B.nextPerformance(director, 0);
  B.updateContext(director, baseContext("waiting"), 50);
  const waiting = B.nextPerformance(director, 50);
  assert.notEqual(waiting, idle);
  assert.equal(waiting.priority, 90);
  assert.equal(waiting.deadlineMs, 5000);
  assert.equal(B.completePerformance(director, idle, 100), false);
  B.updateContext(director, baseContext("dead"), 100);
  const dead = B.nextPerformance(director, 100);
  assert.equal(dead.priority, 100);
  assert.equal(dead.deadlineMs, 8000);
  assert.equal(B.completePerformance(director, waiting, 150), false);
});

test("a rare cooldown is reserved before preemption and stale completion", () => {
  let sample = null;
  for(let seed = 1; seed <= 512 && !sample; seed += 1){
    const director = B.createDirector(`rare-preempt-${seed}`);
    B.updateContext(director, baseContext("idle"), 0);
    const performance = B.nextPerformance(director, 0);
    if(performance.cooldownMs) sample = {director, performance};
  }
  assert.ok(sample, "expected a deterministic first-performance rare sample");
  const {director, performance} = sample;
  const reservedUntil = performance.createdAtMs + performance.cooldownMs;
  assert.equal(director.cooldowns[performance.family], reservedUntil,
    "rare cooldown must arm when composition starts");

  B.notify(director, {type:"tap", target:"preempting-interaction"}, 1);
  const interaction = B.nextPerformance(director, 1);
  assert.equal(interaction.state, "interaction");
  B.updateContext(director, baseContext("waiting"), 2);
  const waiting = B.nextPerformance(director, 2);
  assert.equal(waiting.state, "waiting");
  B.updateContext(director, baseContext("dead"), 3);
  const dead = B.nextPerformance(director, 3);
  assert.equal(dead.state, "dead");
  assert.equal(B.completePerformance(director, performance, 4), false,
    "preempted rare completion is stale");
  assert.equal(B.completePerformance(director, interaction, 4), false);
  assert.equal(B.completePerformance(director, waiting, 4), false);
  assert.equal(director.cooldowns[performance.family], reservedUntil,
    "stale completion must not clear or postpone the reservation");
  assert.equal(B.completePerformance(director, dead, 3 + dead.durationMs), true);

  B.updateContext(director, baseContext("idle"), director.visibleTimeMs);
  for(let index = 0; index < 6 && director.visibleTimeMs < reservedUntil; index += 1){
    const nowMs = director.visibleTimeMs;
    const candidate = B.nextPerformance(director, nowMs);
    assert.notEqual(candidate.family, performance.family,
      "preempted rare became eligible before its reserved deadline");
    B.completePerformance(director, candidate, nowMs + candidate.durationMs);
  }
  assert.equal(director.cooldowns[performance.family], reservedUntil);
});

test("candidate selection never emits a primitive whose support precondition fails", () => {
  const director = B.createDirector("support-aware");
  B.updateContext(director, baseContext("waiting", {supports:{
    "front-left":{mode:"loaded", load:0.5},
    "front-right":{mode:"loaded", load:0.5},
    "rear-left":{mode:"released", load:0},
    "rear-right":{mode:"released", load:0},
  }}), 0);
  const families = [];
  for(let index = 0; index < 4; index += 1){
    const performance = B.nextPerformance(director, index * 10000);
    families.push(performance.family);
    const environment = {context:director.context, supports:director.context.supports,
      drives:director.drives, personality:director.personality};
    for(const phase of performance.phases){
      assert.equal(B.PRIMITIVES[phase.primitive].precondition(environment, phase.params), true,
        `${performance.family}/${phase.primitive} conflicts with loaded support`);
    }
    B.completePerformance(director, performance, index * 10000 + performance.durationMs);
  }
  for(let index = 1; index < families.length; index += 1){
    assert.notEqual(families[index], families[index - 1]);
  }
});

test("recent-family relaxation never selects an invalid interaction fallback", () => {
  const director = B.createDirector("interaction-fallback-support");
  B.updateContext(director, baseContext("idle", {supports:{
    "front-left":{mode:"loaded", load:0.1},
    "front-right":{mode:"loaded", load:0.1},
  }}), 0);
  for(let index = 0; index < 3; index += 1){
    const nowMs = director.visibleTimeMs;
    B.notify(director, {type:"tap", target:`unstable-${index}`}, nowMs);
    const performance = B.nextPerformance(director, nowMs);
    const environment = {context:director.context, supports:director.context.supports,
      drives:director.drives, personality:director.personality};
    for(const phase of performance.phases){
      assert.equal(B.PRIMITIVES[phase.primitive].precondition(environment, phase.params), true,
        `${performance.family}/${phase.primitive} bypassed support rejection`);
    }
    B.completePerformance(director, performance, nowMs + performance.durationMs);
  }
});

test("interactions outrank ordinary states, decay, and habituate", () => {
  const director = B.createDirector("interactions");
  B.updateContext(director, baseContext("working"), 0);
  const before = director.drives.habituation;
  assert.equal(B.notify(director, {type:"pointer", target:"notice", side:"left",
    intensity:1}, 100), true);
  const first = B.nextPerformance(director, 100);
  assert.equal(first.state, "interaction");
  assert.equal(first.priority, 50);
  assert.equal(first.target, "notice");
  assert.equal(B.completePerformance(director, first, 100 + first.durationMs), true);
  const afterFirst = director.drives.habituation;
  assert.ok(afterFirst > before);

  assert.equal(B.notify(director, {type:"pointer", target:"notice", side:"left",
    intensity:1}, 1000), true);
  const repeated = B.nextPerformance(director, 1000);
  assert.ok(repeated.intensity < first.intensity, "habituation must attenuate a repeated notice");
  assert.equal(B.completePerformance(director, repeated, 1000 + repeated.durationMs), true);

  assert.equal(B.notify(director, {type:"pointer", target:"stale"}, 2000), true);
  const decayed = B.nextPerformance(director, 20000);
  assert.equal(decayed.state, "working", "expired interaction must yield to context status");
  assert.equal(decayed.priority, 30);
});

test("same-millisecond interaction ties select the newest bounded insertion", () => {
  const director = B.createDirector("interaction-tie-wrap");
  B.updateContext(director, baseContext("idle"), 1000);
  for(let index = 0; index < 12; index += 1){
    assert.equal(B.notify(director, {type:"tap", target:`notice-${index}`}, 1000), true);
  }
  assert.equal(director.pendingInteractions, 8);
  const targets = [];
  for(let index = 0; index < 8; index += 1){
    const performance = B.nextPerformance(director, 1000);
    targets.push(performance.target);
    B.completePerformance(director, performance, 1000);
  }
  assert.deepEqual(targets, ["notice-11", "notice-10", "notice-9", "notice-8",
    "notice-7", "notice-6", "notice-5", "notice-4"]);
  assert.equal(director.pendingInteractions, 0);
});

test("interaction expiry and repeated-target habituation share the twelve-second boundary", () => {
  function repeatIncrement(elapsedMs){
    const director = B.createDirector(`habituation-boundary-${elapsedMs}`);
    B.updateContext(director, baseContext("idle"), 0);
    B.notify(director, {type:"tap", target:"same", intensity:1}, 0);
    B.updateContext(director, baseContext("idle"), elapsedMs);
    const pending = director.pendingInteractions;
    const before = director.drives.habituation;
    B.notify(director, {type:"tap", target:"same", intensity:1}, elapsedMs);
    return {pending, increment:director.drives.habituation - before};
  }

  const inside = repeatIncrement(11999);
  assert.equal(inside.pending, 1);
  assert.ok(Math.abs(inside.increment - 0.12) < 1e-12);
  const boundary = repeatIncrement(12000);
  assert.equal(boundary.pending, 0);
  assert.ok(Math.abs(boundary.increment - 0.04) < 1e-12);
});

test("a gaze-only interaction honors its loaded-side event target", () => {
  const director = B.createDirector("side-1");
  B.updateContext(director, baseContext("idle", {supports:{
    "front-left":{mode:"loaded", load:1},
    "front-right":{mode:"released", load:0},
  }}), 0);
  B.notify(director, {type:"tap", target:"left-notice", side:"left"}, 0);
  const performance = B.nextPerformance(director, 0);
  assert.equal(performance.family, "interaction-orient");
  assert.equal(performance.side, "left");
});

test("an interaction notified at the saturated visible-time cap is still actionable", () => {
  const director = B.createDirector("huge-time-interaction");
  B.updateContext(director, baseContext("idle"), 1e300);
  assert.equal(B.notify(director, {type:"tap", target:"huge-notice"}, 1e300), true);
  assert.equal(director.pendingInteractions, 1);
  const performance = B.nextPerformance(director, 1e300);
  assert.equal(performance.state, "interaction");
  assert.equal(performance.target, "huge-notice");
});

test("an interaction expires exactly twelve seconds before the visible-time cap", () => {
  const director = B.createDirector("cap-expiry");
  const createdAtMs = Number.MAX_SAFE_INTEGER - 12000;
  B.updateContext(director, baseContext("idle"), createdAtMs);
  B.notify(director, {type:"tap", target:"expiring-notice"}, createdAtMs);
  B.updateContext(director, baseContext("idle"), Number.MAX_SAFE_INTEGER);
  assert.equal(director.pendingInteractions, 0);
  assert.equal(B.nextPerformance(director, Number.MAX_SAFE_INTEGER).state, "idle");
});

test("completion notices are idempotent and only complete the active performance", () => {
  const director = B.createDirector("completion");
  B.updateContext(director, baseContext(), 0);
  const performance = B.nextPerformance(director, 0);
  const satisfaction = director.drives.satisfaction;
  assert.equal(B.notify(director, {type:"completion", performance},
    performance.durationMs), true);
  assert.equal(director.completions, 1);
  assert.ok(director.drives.satisfaction > satisfaction);
  assert.equal(B.completePerformance(director, performance, performance.durationMs + 1), false);

  const other = collect("other", 1).performances[0];
  assert.equal(B.completePerformance(director, other, 10000), false);
});

test("family, side, and target memories wrap at exactly eight entries", () => {
  const {director, performances} = collect("memory-wrap", 40);
  for(let index = 1; index < performances.length; index += 1){
    assert.notEqual(performances[index].family, performances[index - 1].family,
      `family repeated at ${index}`);
  }
  assert.deepEqual(director.memory.families,
    performances.slice(-8).map(performance => performance.family));
  assert.deepEqual(director.memory.sides,
    performances.slice(-8).map(performance => performance.side));
  assert.deepEqual(director.memory.targets,
    performances.slice(-8).map(performance => performance.target));
  assert.equal(Object.isFrozen(director.memory), true);
  assert.equal(Object.isFrozen(director.memory.families), true);
});

test("rare families obey their declared cooldowns", () => {
  const {performances} = collect("rare-cooldown", 320);
  const lastAt = new Map();
  let rareCount = 0;
  for(const performance of performances){
    if(!performance.cooldownMs) continue;
    rareCount += 1;
    const previous = lastAt.get(performance.family);
    if(previous !== undefined){
      assert.ok(performance.createdAtMs - previous >= performance.cooldownMs,
        `${performance.family} repeated inside ${performance.cooldownMs}ms`);
    }
    lastAt.set(performance.family, performance.createdAtMs);
  }
  assert.ok(rareCount >= 4, `only ${rareCount} rare performances sampled`);
});

test("signatures encode semantics but ignore session, clock, and object identity", () => {
  const performance = collect("signature", 1).performances[0];
  const clone = JSON.parse(JSON.stringify(performance));
  clone.createdAtMs += 123456;
  clone.sessionId = "another-session";
  clone.directorId = "fake-raw-id";
  assert.equal(B.performanceSignature(performance), B.performanceSignature(clone));

  const durationChange = JSON.parse(JSON.stringify(performance));
  durationChange.phases[0].durationMs += 1;
  assert.notEqual(B.performanceSignature(performance), B.performanceSignature(durationChange));
  const targetChange = JSON.parse(JSON.stringify(performance));
  targetChange.phases[0].targets.__semanticProbe = 0.125;
  assert.notEqual(B.performanceSignature(performance), B.performanceSignature(targetChange));
});

test("grammar reaches ten thousand valid idle signatures", () => {
  const signatures = new Set();
  for(let seed = 1; seed <= 512 && signatures.size < 10000; seed += 1){
    const director = B.createDirector(seed);
    B.updateContext(director, {sessionId:`s-${seed}`, status:"idle", role:"daily",
      costume:"", contextPressure:"low", theme:"noche", expanded:false}, 0);
    for(let index = 0; index < 96; index += 1){
      const nowMs = index * 61000;
      const performance = B.nextPerformance(director, nowMs);
      assert.ok(performance.phases.every(phase =>
        phase.durationMs > 0 && phase.safeEnd !== undefined));
      signatures.add(B.performanceSignature(performance));
      B.completePerformance(director, performance, nowMs + performance.durationMs);
    }
  }
  assert.ok(signatures.size >= 10000, `only ${signatures.size} signatures`);
});

test("director state remains bounded under sustained updates and notifications", () => {
  const director = B.createDirector("bounded");
  const keys = Object.keys(director);
  for(let index = 0; index < 2000; index += 1){
    B.updateContext(director, baseContext("idle"), index * 17);
    B.notify(director, {type:"pointer", target:`target-${index}`, side:index % 2 ? "left" : "right"},
      index * 17);
    if(index % 7 === 0){
      const performance = B.nextPerformance(director, index * 17);
      B.completePerformance(director, performance, index * 17 + performance.durationMs);
    }
  }
  assert.deepEqual(Object.keys(director), keys);
  assert.ok(director.pendingInteractions <= 8);
  assert.ok(director.memory.families.length <= 8);
  assert.ok(director.memory.sides.length <= 8);
  assert.ok(director.memory.targets.length <= 8);
  assert.ok(Object.keys(director.cooldowns).length <= 4);
  for(const value of Object.values(director.drives)){
    assert.ok(Number.isFinite(value) && value >= 0 && value <= 1);
  }
});
