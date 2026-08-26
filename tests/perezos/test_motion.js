"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");

global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");
require("../../dash/perezos/rig.js");
require("../../dash/perezos/behaviors.js");
require("../../dash/perezos/motion.js");

const C = global.ComandOSPerezOS.Core;
const R = global.ComandOSPerezOS.Rig;
const B = global.ComandOSPerezOS.Behaviors;
const M = global.ComandOSPerezOS.Motion;
const MOTION_SOURCE = fs.readFileSync(require.resolve("../../dash/perezos/motion.js"), "utf8");

function frozenPhase(primitive, durationMs, safeEnd, targets, overrides = {}){
  return Object.freeze({
    primitive,
    params:Object.freeze({side:"left", target:"test", distance:0.5, grip:0.7,
      intensity:0.7, headLead:0.5, furFollowThrough:0.5,
      phaseIndex:overrides.phaseIndex || 0}),
    targets:Object.freeze({...targets}),
    actionMs:overrides.actionMs || durationMs,
    pauseAfterMs:durationMs - (overrides.actionMs || durationMs),
    durationMs,
    safeEnd,
    interruptible:overrides.interruptible !== false,
    startMs:overrides.startMs || 0,
    endMs:(overrides.startMs || 0) + durationMs,
  });
}

function frozenPerformance(family, priority, phases, overrides = {}){
  const durationMs = phases.reduce((sum, phase) => sum + phase.durationMs, 0);
  return Object.freeze({
    state:overrides.state || family,
    family,
    priority,
    deadlineMs:overrides.deadlineMs === undefined ? null : overrides.deadlineMs,
    side:overrides.side || "left",
    target:overrides.target || "test",
    distance:0.5,
    grip:0.7,
    intensity:0.7,
    headLead:0.5,
    furFollowThrough:0.5,
    pauseMs:phases.reduce((sum, phase) => sum + phase.pauseAfterMs, 0),
    cooldownMs:0,
    durationMs,
    createdAtMs:overrides.createdAtMs || 0,
    sessionId:"motion-test",
    phases:Object.freeze(phases),
  });
}

function makePhase(primitive, durationMs, safeEnd, targets, overrides = {}){
  return frozenPhase(primitive, durationMs, safeEnd, targets, overrides);
}

function simplePerformance(family = "idle-a", priority = 10, durationMs = 400,
    overrides = {}){
  const primitive = overrides.primitive || "turn-head";
  const targets = overrides.targets || {"head-yaw":0.5};
  const phase = makePhase(primitive, durationMs,
    overrides.safeEnd === undefined ? Math.round(durationMs * 0.6) : overrides.safeEnd,
    targets, {interruptible:overrides.interruptible,
      actionMs:overrides.actionMs, phaseIndex:0});
  return frozenPerformance(family, priority, [phase], overrides);
}

function twoPhasePerformance(){
  const first = makePhase("turn-head", 1000, 600, {"head-yaw":0.6}, {phaseIndex:0});
  const second = makePhase("turn-head", 1000, 600, {"head-yaw":-0.4},
    {phaseIndex:1, startMs:1000});
  return frozenPerformance("ordered", 10, [first, second], {state:"idle"});
}

function urgentPerformance(family, priority, deadlineMs){
  return simplePerformance(family, priority, 500, {state:family, deadlineMs,
    primitive:family === "dead" ? "settle" : "perceive",
    targets:family === "dead" ? {"body-lean-x":0, "body-lift":0} :
      {"brow-left-lift":0.4, "brow-right-lift":0.4}});
}

function fixture(seed = "motion-fixture"){
  const rig = R.createRig(seed);
  return {rig, motion:M.createMotion(rig)};
}

function unreachableRightRig(seed){
  const rig = R.createRig(seed);
  assert.equal(R.requestGrip(rig, "front-left", "loaded", 0.34), true);
  for(let frame = 0; frame < 300; frame += 1){
    assert.equal(R.solveRig(rig, 1 / 120), true);
  }
  assert.equal(R.requestGrip(rig, "rear-right", "release", 0.72), true);
  const unreachablePose = {
    "cable-sway":R.LIMITS["cable-sway"].min,
    "cable-lift":R.LIMITS["cable-lift"].min,
    "cable-tension":R.LIMITS["cable-tension"].min,
    "cable-wind":R.LIMITS["cable-wind"].min,
    "cable-pulse":R.LIMITS["cable-pulse"].min,
    "body-lean-x":R.LIMITS["body-lean-x"].max,
    "body-lift":R.LIMITS["body-lift"].min,
  };
  for(const [channel, target] of Object.entries(unreachablePose)){
    assert.equal(R.setChannelTarget(rig, channel, target), true);
  }
  for(let frame = 0; frame < 1200; frame += 1){
    assert.equal(R.solveRig(rig, 1 / 120), true);
  }
  assert.equal(rig.supports["front-right"].mode, "released");
  assert.equal(R.requestGrip(rig, "front-right", "loaded", 0.72), false,
    "fixture must exercise a real unreachable Rig contact");
  assert.deepEqual(R.validatePose(rig), []);
  return rig;
}

function stepFrames(motion, frames, dt = 1 / 60, startMs = 0){
  for(let frame = 1; frame <= frames; frame += 1){
    assert.equal(M.stepMotion(motion, dt, startMs + (frame - 1) * dt * 1000), true,
      `step ${frame}`);
  }
}

test("motion namespace exposes the exact scheduler API", () => {
  assert.deepEqual(Object.keys(M), [
    "createMotion", "enqueue", "requestInterrupt", "stepMotion", "isIdle",
  ]);
  for(const name of Object.keys(M)) assert.equal(typeof M[name], "function", name);
});

test("motion preallocates typed ownership and causal state for every rig channel", () => {
  const {motion, rig} = fixture();
  const buffers = ["owners", "channelTargets", "phaseStarts", "baseTargets",
    "previousVelocities", "accelerations"];
  for(const name of buffers){
    const expected = name === "owners" ? Int16Array : Float64Array;
    assert.ok(motion[name] instanceof expected, name);
    assert.equal(motion[name].length, R.CHANNELS.length, name);
    assert.equal(Object.isSealed(motion[name]), true, `${name} must retain its shape`);
  }
  assert.deepEqual(Array.from(motion.owners), new Array(120).fill(-1));
  assert.ok(motion.queueTerminalDeadlines instanceof Float64Array);
  assert.equal(motion.queueTerminalDeadlines.length, 8);
  assert.equal(Object.isSealed(motion.queueTerminalDeadlines), true);
  assert.equal(Array.from(motion.queueTerminalDeadlines).every(Number.isFinite), true);
  assert.equal(motion.rig, rig);
  assert.equal(Object.isSealed(motion), true);
  assert.equal(M.isIdle(motion), true);
});

test("motion construction avoids Chrome-incompatible sealing of populated typed arrays", () => {
  const rig = R.createRig("chrome-motion-typed-array-contract");
  const nativeSeal = Object.seal;
  Object.seal = value => {
    if(ArrayBuffer.isView(value) && value.byteLength > 0){
      throw new TypeError("Cannot seal array buffer views with elements");
    }
    return nativeSeal(value);
  };
  try{
    let motion;
    assert.doesNotThrow(() => { motion = M.createMotion(rig); });
    assert.equal(Object.isExtensible(motion.owners), false);
    assert.equal(Reflect.defineProperty(motion.owners, "custom", {value:true}), false);
    motion.owners[0] = 7;
    assert.equal(motion.owners[0], 7, "motion buffers must keep writable elements");
  }finally{
    Object.seal = nativeSeal;
  }
});

test("phase targets progress in order with smoothstep blending", () => {
  const {motion, rig} = fixture("smooth-order");
  const yaw = R.channelIndex("head-yaw");
  assert.equal(M.enqueue(motion, twoPhasePerformance(), 0), true);
  assert.equal(motion.phaseIndex, 0);

  assert.equal(M.stepMotion(motion, 0.25, 250), true);
  assert.ok(Math.abs(rig.targets[yaw] - C.lerp(0, 0.6, C.smoothstep(0.25))) < 1e-12);
  M.stepMotion(motion, 0.25, 500);
  assert.ok(Math.abs(rig.targets[yaw] - 0.3) < 1e-12);
  M.stepMotion(motion, 0.25, 750);
  M.stepMotion(motion, 0.25, 1000);
  assert.equal(motion.phaseIndex, 1);
  assert.ok(Math.abs(rig.targets[yaw] - 0.6) < 1e-12);

  M.stepMotion(motion, 0.5, 1500);
  assert.ok(Math.abs(rig.targets[yaw] - 0.1) < 1e-12);
  assert.deepEqual(R.validatePose(rig), []);
});

test("a phase owns only its declared finite target channels with no double ownership", () => {
  const {motion} = fixture("ownership");
  const performance = simplePerformance("ownership", 10, 1000, {targets:{
    "head-yaw":0.4, "fur-head-crest":0.2,
  }});
  assert.equal(M.enqueue(motion, performance, 0), true);
  const claimed = [];
  for(let index = 0; index < motion.owners.length; index += 1){
    if(motion.owners[index] !== -1) claimed.push(R.CHANNELS[index]);
    assert.ok(motion.owners[index] === -1 || motion.owners[index] === 0,
      `${R.CHANNELS[index]} has invalid owner ${motion.owners[index]}`);
  }
  assert.deepEqual(claimed, ["head-yaw", "fur-head-crest"]);

  const undeclared = simplePerformance("undeclared", 10, 200, {
    primitive:"turn-head", targets:{"jaw-open":0.8},
  });
  const another = fixture("undeclared").motion;
  assert.equal(M.enqueue(another, undeclared, 0), false);
  assert.equal(M.isIdle(another), true);
});

test("ordinary enqueue is fixed-capacity and preserves performance order", () => {
  const {motion} = fixture("queue-order");
  const performances = Array.from({length:10}, (_, index) =>
    simplePerformance(`queued-${index}`, 10, 50));
  assert.equal(M.enqueue(motion, performances[0], 0), true);
  for(let index = 1; index <= 8; index += 1){
    assert.equal(M.enqueue(motion, performances[index], 0), true, `queue ${index}`);
  }
  assert.equal(M.enqueue(motion, performances[9], 0), false, "ninth queued item is rejected");
  const observed = [];
  let previous = null;
  for(let frame = 0; frame < 200 && !M.isIdle(motion); frame += 1){
    if(motion.current !== previous){
      observed.push(motion.current.family);
      previous = motion.current;
    }
    M.stepMotion(motion, 0.05, frame * 50);
  }
  assert.deepEqual(observed, performances.slice(0, 9).map(item => item.family));
});

test("higher priority interruption cuts at an interruptible safe boundary", () => {
  const {motion, rig} = fixture("safe-cut");
  const active = simplePerformance("active", 10, 2000,
    {safeEnd:400, interruptible:true, targets:{"head-yaw":0.7}});
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, active, 0);
  M.stepMotion(motion, 0.1, 0);
  assert.equal(M.requestInterrupt(motion, waiting, 100), true);
  let started = null;
  for(let ms = 100; ms <= 1000; ms += 16){
    M.stepMotion(motion, 0.016, ms);
    if(motion.current === waiting){
      started = motion.completions.at(-1).endedAtMs;
      break;
    }
  }
  assert.equal(started, 400);
  assert.equal(motion.transitionPhase, null, "gaze cut does not need an anatomical bridge");
  assert.deepEqual(R.validatePose(rig), []);
});

test("a non-interruptible anatomical phase cuts only at its declared safeEnd", () => {
  const {motion, rig} = fixture("non-interruptible-safe-end");
  const active = simplePerformance("weighted-reach", 10, 2000, {
    primitive:"reach", safeEnd:700, interruptible:false,
    targets:{"front-left-reach-x":6, "front-left-reach-y":-4},
  });
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, active, 0);
  M.stepMotion(motion, 0.1, 0);
  M.requestInterrupt(motion, waiting, 100);
  let started = null;
  for(let ms = 100; ms <= 1000; ms += 20){
    M.stepMotion(motion, 0.02, ms);
    if(motion.current === waiting){
      started = motion.completions.at(-1).endedAtMs;
      break;
    }
  }
  assert.equal(started, 700);
  assert.equal(motion.transitionPhase, "brace",
    "non-interruptible anatomy uses the safe bridge after safeEnd");
  assert.deepEqual(R.validatePose(rig), []);
});

test("a phase boundary crossed within one step is not lost to the next phase", () => {
  const {motion, rig} = fixture("crossed-phase-boundary");
  const first = makePhase("turn-head", 100, 90, {"head-yaw":0.4},
    {phaseIndex:0, interruptible:false});
  const second = makePhase("turn-head", 2000, 1800, {"head-yaw":-0.4},
    {phaseIndex:1, startMs:100, interruptible:false});
  const active = frozenPerformance("two-boundaries", 10, [first, second],
    {state:"idle"});
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, active, 0);
  M.requestInterrupt(motion, waiting, 0);
  M.stepMotion(motion, 0.1, 0);
  assert.equal(motion.current, waiting,
    "the completed first phase is itself a safe interruption boundary");
  assert.equal(motion.completions.at(-1).endedAtMs, 90);
  assert.deepEqual(R.validatePose(rig), []);
});

test("a pending interrupt at the final phase boundary records natural completion", () => {
  const {motion} = fixture("final-boundary-completion");
  const active = simplePerformance("finishing", 10, 100, {
    safeEnd:100, interruptible:false,
  });
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, active, 0);
  M.requestInterrupt(motion, waiting, 0);
  M.stepMotion(motion, 0.1, 0);
  assert.equal(motion.current, waiting, "pending work starts at the final boundary");
  assert.deepEqual(motion.completions.at(-1), {
    status:"completed", state:"finishing", family:"finishing", startedAtMs:0,
    endedAtMs:100, phaseCount:1, completedPhases:1, interruptedBy:null,
  });
});

test("urgent waiting interruption cannot miss its five second deadline", () => {
  const {rig, motion} = fixture("waiting-deadline");
  const scratch = simplePerformance("long-scratch", 10, 30000, {
    primitive:"scratch", safeEnd:29000, interruptible:false,
    targets:{"front-left-wrist-angle":0.5, "fur-head-cheek-left":0.6},
  });
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, scratch, 0);
  M.requestInterrupt(motion, waiting, 1000);
  let started = null;
  for(let ms = 1000; ms < 6000; ms += 16){
    M.stepMotion(motion, 0.016, ms);
    if(motion.current && motion.current.family === "waiting"){
      started = motion.completions.at(-1).endedAtMs;
      break;
    }
  }
  assert.equal(started, 6000);
  assert.equal(motion.transitionPhase, "brace");
  let sawSettle = false;
  for(let frame = 1; frame <= 30; frame += 1){
    M.stepMotion(motion, 0.016, started + frame * 16);
    if(motion.transitionPhase === "settle") sawSettle = true;
  }
  assert.equal(sawSettle, true, "hazardous cut inserts brace then settle");
  assert.deepEqual(R.validatePose(rig), []);
});

test("dead interruption reaches its current performance within eight seconds", () => {
  const {motion, rig} = fixture("dead-deadline");
  M.enqueue(motion, simplePerformance("uninterruptible", 20, 40000, {
    primitive:"pull", safeEnd:39000, interruptible:false,
    targets:{"body-lean-x":-4, "front-left-elbow-angle":0.5,
      "cable-load-scale":1.2},
  }), 0);
  const dead = urgentPerformance("dead", 100, 8000);
  M.requestInterrupt(motion, dead, 500);
  let started = null;
  for(let ms = 500; ms <= 8500; ms += 20){
    M.stepMotion(motion, 0.02, ms);
    if(motion.current === dead){ started = ms; break; }
  }
  assert.ok(started !== null && started <= 8500, `dead started ${started}`);
  assert.deepEqual(R.validatePose(rig), []);
});

test("multi-phase dead carries leftover dt and completes its safe pose by eight seconds", () => {
  const {motion, rig} = fixture("dead-safe-pose-deadline");
  M.enqueue(motion, simplePerformance("long-hazard", 10, 30000, {
    primitive:"pull", safeEnd:29000, interruptible:false,
    targets:{"body-lean-x":-3, "front-left-elbow-angle":0.4,
      "cable-load-scale":1.15},
  }), 0);
  const deadPhases = [
    makePhase("perceive", 333, 220,
      {"brow-left-lift":0.4, "brow-right-lift":0.4}, {phaseIndex:0}),
    makePhase("turn-head", 333, 220, {"head-yaw":0.2},
      {phaseIndex:1, startMs:333}),
    makePhase("close-grip", 333, 300, {
      "claw-front-left-1-curl":0.75,
      "claw-front-left-2-curl":0.75,
      "claw-front-left-3-curl":0.75,
    }, {phaseIndex:2, startMs:666, interruptible:false}),
    makePhase("settle", 333, 220, {"body-lean-x":0, "body-lift":0},
      {phaseIndex:3, startMs:999}),
  ];
  const dead = frozenPerformance("dead", 100, deadPhases,
    {state:"dead", deadlineMs:8000});
  M.requestInterrupt(motion, dead, 0);
  for(let ms = 0; ms <= 8000 &&
      !motion.completions.some(record => record.family === "dead" &&
        record.status === "completed"); ms += 50){
    M.stepMotion(motion, 0.05, ms);
  }
  const record = motion.completions.find(item => item.family === "dead" &&
    item.status === "completed");
  assert.ok(record, "dead performance did not reach its safe completed pose");
  assert.ok(record.endedAtMs <= 8000, `dead completed at ${record.endedAtMs}`);
  assert.deepEqual(R.validatePose(rig), []);
});

test("dead falls back safely when its released contact is unreachable at the terminal deadline", () => {
  const rig = unreachableRightRig("dead-unreachable-contact");

  const motion = M.createMotion(rig);
  const active = simplePerformance("pre-dead-hazard", 10, 30000, {
    primitive:"pull", safeEnd:29000, interruptible:false,
    targets:{"body-lean-x":-3, "front-left-elbow-angle":0.4,
      "cable-load-scale":1.15},
  });
  const close = makePhase("close-grip", 1000, 1000, {
    "claw-front-right-1-curl":0.8,
    "claw-front-right-2-curl":0.8,
    "claw-front-right-3-curl":0.8,
  }, {phaseIndex:0, interruptible:false});
  const dead = frozenPerformance("dead-unreachable", 100, [close], {
    state:"dead", side:"right", deadlineMs:8000,
  });
  M.enqueue(motion, active, 5000);
  M.requestInterrupt(motion, dead, 5000);
  assert.equal(M.stepMotion(motion, 10, 1000), true,
    "a rewound large step crosses both the cut and terminal deadline");
  const terminal = motion.completions.filter(record =>
    record.family === "dead-unreachable");
  assert.equal(terminal.length, 1, "dead emits one terminal record");
  assert.equal(terminal[0].status, "completed");
  assert.equal(terminal[0].endedAtMs, 13000);
  assert.equal(rig.supports["front-left"].mode, "loaded");
  assert.equal(rig.supports["front-left"].load, 1);
  assert.equal(rig.supports["front-right"].mode, "released",
    "fallback must not force an impossible contact");
  assert.deepEqual(R.validatePose(rig), []);

  const after = simplePerformance("after-dead-fallback", 10, 1000);
  assert.equal(M.enqueue(motion, after, 15000), true);
  M.stepMotion(motion, 0.1, 15000);
  assert.equal(motion.current, after,
    "the cleared terminal deadline cannot complete later work");
});

test("an ordinarily queued dead performance is promoted before its absolute deadline", () => {
  const rig = unreachableRightRig("queued-dead-unreachable-contact");
  const motion = M.createMotion(rig);
  const active = simplePerformance("queued-pre-dead", 10, 20000, {
    primitive:"pull", safeEnd:19000, interruptible:false,
    targets:{"body-lean-x":-3, "front-left-elbow-angle":0.4,
      "cable-load-scale":1.15},
  });
  const close = makePhase("close-grip", 1000, 1000, {
    "claw-front-right-1-curl":0.8,
    "claw-front-right-2-curl":0.8,
    "claw-front-right-3-curl":0.8,
  }, {phaseIndex:0, interruptible:false});
  const dead = frozenPerformance("queued-dead", 100, [close], {
    state:"dead", side:"right", deadlineMs:8000,
  });
  M.enqueue(motion, active, 0);
  assert.equal(M.enqueue(motion, dead, 0), true);
  assert.equal(motion.queued, 1, "ordinary enqueue initially uses the fixed queue");
  assert.equal(motion.queueTerminalDeadlines[0], 8000);
  assert.equal(M.stepMotion(motion, 8, 0), true);
  const terminal = motion.completions.filter(record => record.family === "queued-dead");
  assert.equal(terminal.length, 1);
  assert.equal(terminal[0].status, "completed");
  assert.equal(terminal[0].endedAtMs, 8000);
  assert.equal(rig.supports["front-left"].mode, "loaded");
  assert.equal(rig.supports["front-right"].mode, "released");
  assert.deepEqual(R.validatePose(rig), []);
});

test("an interrupt deadline still forces a cut while contact transfer holds the phase", () => {
  const {motion, rig} = fixture("contact-held-deadline");
  const release = simplePerformance("release-held", 10, 30000, {
    primitive:"release", side:"left", safeEnd:29000, interruptible:false,
    targets:{
      "claw-front-left-1-curl":0.05,
      "claw-front-left-2-curl":0.05,
      "claw-front-left-3-curl":0.05,
    },
  });
  const waiting = urgentPerformance("waiting", 90, 100);
  M.enqueue(motion, release, 0);
  M.requestInterrupt(motion, waiting, 0);
  M.stepMotion(motion, 0.05, 0);
  assert.equal(motion.current, release, "contact phase is initially held for transfer");
  M.stepMotion(motion, 0.05, 50);
  assert.equal(motion.current, waiting, "deadline wins even while contact is held");
  assert.equal(motion.completions.at(-1).endedAtMs, 100);
  assert.deepEqual(R.validatePose(rig), []);
});

test("finite timestamp rewind cannot move deadlines or completion records backward", () => {
  const {motion} = fixture("monotonic-motion-time");
  const active = simplePerformance("clock-active", 10, 30000, {
    primitive:"scratch", safeEnd:29000, interruptible:false,
    targets:{"front-left-wrist-angle":0.4, "fur-head-cheek-left":0.4},
  });
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, active, 5000);
  assert.equal(M.requestInterrupt(motion, waiting, 1000), true,
    "valid rewind is accepted without rewinding the visible clock");
  M.stepMotion(motion, 0.01, 6000);
  assert.equal(motion.current, active,
    "deadline is five seconds after monotonic request time, not rewound input");
  M.stepMotion(motion, 0.01, 9995);
  assert.equal(motion.current, waiting);
  const interruption = motion.completions.at(-1);
  assert.ok(interruption.endedAtMs >= interruption.startedAtMs);

  const short = simplePerformance("clock-complete", 10, 10);
  const another = fixture("monotonic-completion").motion;
  M.enqueue(another, short, 5000);
  M.stepMotion(another, 0.01, 1000);
  assert.equal(another.completions[0].endedAtMs, 5010);
});

test("lower or equal priority cannot replace active or pending work", () => {
  const {motion} = fixture("interrupt-priority");
  M.enqueue(motion, simplePerformance("active", 50, 1000), 0);
  assert.equal(M.requestInterrupt(motion, simplePerformance("lower", 40), 10), false);
  const high = simplePerformance("high", 90);
  assert.equal(M.requestInterrupt(motion, high, 10), true);
  assert.equal(M.requestInterrupt(motion, simplePerformance("equal", 90), 11), false);
  assert.equal(M.requestInterrupt(motion, simplePerformance("middle", 80), 12), false);
  assert.equal(motion.pendingInterrupt, high);
});

test("release transfers support before opening the loaded claw", () => {
  const {motion, rig} = fixture("support-transfer");
  const curl = R.channelIndex("claw-front-left-1-curl");
  const initialCurl = rig.targets[curl];
  const release = simplePerformance("release-left", 40, 800, {
    primitive:"release", side:"left", targets:{
      "claw-front-left-1-curl":0.05,
      "claw-front-left-2-curl":0.05,
      "claw-front-left-3-curl":0.05,
    },
  });
  M.enqueue(motion, release, 0);
  let releasedAt = null;
  for(let frame = 1; frame <= 120; frame += 1){
    const beforeMode = rig.supports["front-left"].mode;
    const safeLoad = rig.supports["rear-right"].load;
    M.stepMotion(motion, 1 / 60, frame * 1000 / 60);
    if(beforeMode === "loaded" && safeLoad < 0.95){
      assert.equal(rig.targets[curl], initialCurl,
        "opening target must wait for a safe alternate support");
    }
    if(rig.supports["front-left"].mode === "released"){
      releasedAt = frame;
      assert.ok(rig.supports["rear-right"].load >= 0.95);
      break;
    }
    assert.deepEqual(R.validatePose(rig), []);
  }
  assert.notEqual(releasedAt, null, "loaded claw eventually releases");
  assert.equal(rig.supports["front-left"].load, 0);
  stepFrames(motion, 10, 1 / 60, releasedAt * 1000 / 60);
  assert.ok(rig.targets[curl] < initialCurl, "opening begins after release");
  assert.deepEqual(R.validatePose(rig), []);
});

test("close-grip establishes support before applying claw targets", () => {
  const {motion, rig} = fixture("close-contact");
  const close = simplePerformance("close-right", 30, 500, {
    primitive:"close-grip", side:"right", targets:{
      "claw-front-right-1-curl":0.8,
      "claw-front-right-2-curl":0.8,
      "claw-front-right-3-curl":0.8,
    },
  });
  assert.equal(rig.supports["front-right"].mode, "released");
  M.enqueue(motion, close, 0);
  M.stepMotion(motion, 1 / 60, 16);
  assert.equal(rig.supports["front-right"].mode, "loaded");
  assert.equal(rig.supports["front-right"].cableT, 0.72);
  assert.ok(rig.targets[R.channelIndex("claw-front-right-1-curl")] > 0.45);
  assert.deepEqual(R.validatePose(rig), []);
});

test("touch and comfort primitives never invent load-bearing contacts", () => {
  for(const primitive of ["touch", "comfort-cable"]){
    const {motion, rig} = fixture(`non-contact-${primitive}`);
    const performance = simplePerformance(primitive, 20, 200, {
      primitive, side:"right", targets:primitive === "touch" ?
        {"front-right-reach-x":4, "claw-front-right-1-curl":0.3} :
        {"front-right-wrist-angle":-0.1, "cable-pulse":0.2},
    });
    M.enqueue(motion, performance, 0);
    M.stepMotion(motion, 1 / 60, 16);
    assert.equal(rig.supports["front-right"].mode, "released", primitive);
    assert.deepEqual(R.validatePose(rig), [], primitive);
  }
});

test("completion callback and bounded records expose deeply immutable snapshots", () => {
  const {motion} = fixture("completion-records");
  const callbacks = [];
  motion.onComplete = record => callbacks.push(record);
  const performance = twoPhasePerformance();
  M.enqueue(motion, performance, 100);
  stepFrames(motion, 8, 0.25, 100);
  assert.equal(M.isIdle(motion), true);
  assert.equal(callbacks.length, 1);
  assert.deepEqual(callbacks[0], {
    status:"completed", state:"idle", family:"ordered", startedAtMs:100,
    endedAtMs:2100, phaseCount:2, completedPhases:2, interruptedBy:null,
  });
  assert.equal(Object.isFrozen(callbacks[0]), true);
  const firstSnapshot = motion.completions;
  assert.equal(Object.isFrozen(firstSnapshot), true);
  assert.equal(Object.isFrozen(firstSnapshot[0]), true);
  assert.notEqual(motion.completions, firstSnapshot, "public snapshots cannot mutate the ring");

  for(let index = 0; index < 24; index += 1){
    M.enqueue(motion, simplePerformance(`record-${index}`, 10, 25), 2200 + index * 25);
    M.stepMotion(motion, 0.025, 2225 + index * 25);
  }
  assert.equal(motion.completions.length, 16, "completion history is fixed-capacity");
  assert.equal(motion.completionCount, 25);
});

test("natural completion installs its successor before a callback re-enters stepping", () => {
  const {motion, rig} = fixture("callback-step-completion");
  const {motion:controlMotion, rig:controlRig} =
    fixture("callback-step-completion");
  const finishing = simplePerformance("finishing", 10, 100, {safeEnd:100});
  const successor = simplePerformance("successor", 10, 1000, {
    targets:{"head-yaw":-0.5},
  });
  let callbackCount = 0;
  let observedCurrent = null;
  let nestedResult = null;
  M.enqueue(motion, finishing, 0);
  M.enqueue(motion, successor, 0);
  M.enqueue(controlMotion, finishing, 0);
  M.enqueue(controlMotion, successor, 0);
  assert.equal(M.stepMotion(controlMotion, 0.2, 0), true);
  motion.onComplete = () => {
    callbackCount += 1;
    if(callbackCount !== 1) return;
    observedCurrent = motion.current;
    nestedResult = M.stepMotion(motion, 0.01, 150);
    throw new Error("contained callback failure");
  };
  const solvesBefore = rig.diagnostics.steps;
  assert.equal(M.stepMotion(motion, 0.2, 0), true);
  assert.equal(observedCurrent, successor,
    "callback must observe the already-installed queued successor");
  assert.equal(nestedResult, true);
  assert.equal(callbackCount, 1);
  assert.equal(motion.completionCount, 1,
    "reentrant stepping cannot complete the same performance twice");
  assert.equal(motion.current, successor);
  assert.equal(motion.phaseIndex, 0);
  assert.ok(Math.abs(rig.targets[R.channelIndex("head-yaw")] -
    C.lerp(0.5, -0.5, C.smoothstep(0.1))) < 1e-12,
  "outer remainder must advance the successor through the full 100ms union");
  assert.equal(motion.completions[0].endedAtMs, 100);
  assert.equal(rig.diagnostics.steps, solvesBefore + 1,
    "the reentrant transaction performs one Rig solve for the full union");
  assert.equal(R.poseHash(rig), R.poseHash(controlRig),
    "one full-union solve must match the non-reentrant physical result");
});

test("interrupted work records a terminal causal snapshot", () => {
  const {motion} = fixture("interrupted-record");
  const active = simplePerformance("old", 10, 3000, {safeEnd:100});
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, active, 0);
  M.stepMotion(motion, 0.1, 0);
  M.requestInterrupt(motion, waiting, 100);
  M.stepMotion(motion, 0.01, 100);
  const record = motion.completions.at(-1);
  assert.equal(record.status, "interrupted");
  assert.equal(record.family, "old");
  assert.equal(record.interruptedBy, "waiting");
  assert.equal(record.completedPhases, 0);
  assert.equal(Object.isFrozen(record), true);
});

test("interruption installs pending work before a callback re-enters stepping", () => {
  const {motion, rig} = fixture("callback-step-interruption");
  const {motion:controlMotion, rig:controlRig} =
    fixture("callback-step-interruption");
  const active = simplePerformance("interrupted-once", 10, 1000, {safeEnd:100});
  const waiting = urgentPerformance("waiting", 90, 5000);
  let callbackCount = 0;
  let observedCurrent = null;
  M.enqueue(motion, active, 0);
  M.requestInterrupt(motion, waiting, 0);
  M.enqueue(controlMotion, active, 0);
  M.requestInterrupt(controlMotion, waiting, 0);
  assert.equal(M.stepMotion(controlMotion, 0.2, 0), true);
  motion.onComplete = record => {
    callbackCount += 1;
    if(record.status === "interrupted" && callbackCount === 1){
      observedCurrent = motion.current;
      assert.equal(M.stepMotion(motion, 0.01, 150), true);
    }
  };
  const solvesBefore = rig.diagnostics.steps;
  assert.equal(M.stepMotion(motion, 0.2, 0), true);
  assert.equal(observedCurrent, waiting,
    "callback must observe the already-installed interrupt successor");
  assert.equal(callbackCount, 1);
  assert.equal(motion.completionCount, 1);
  assert.equal(motion.completions[0].family, "interrupted-once");
  assert.equal(motion.completions[0].endedAtMs, 100);
  assert.equal(motion.current, waiting);
  assert.ok(Math.abs(rig.targets[R.channelIndex("brow-left-lift")] -
    0.4 * C.smoothstep(0.2)) < 1e-12,
  "outer remainder must advance interrupted successor through the full union");
  assert.equal(rig.diagnostics.steps, solvesBefore + 1,
    "the interrupted reentrant transaction performs one full-union Rig solve");
  assert.equal(R.poseHash(rig), R.poseHash(controlRig),
    "one full-union solve must match the non-reentrant physical result");
});

test("an invalid nested step cannot steal the outer schedule or Rig solve", () => {
  const {motion, rig} = fixture("callback-invalid-step");
  const {motion:controlMotion, rig:controlRig} = fixture("callback-invalid-step");
  const finishing = simplePerformance("invalid-step-finishing", 10, 100,
    {safeEnd:100});
  const successor = simplePerformance("invalid-step-successor", 10, 1000, {
    targets:{"head-yaw":-0.5},
  });
  let invalidResult = null;
  M.enqueue(motion, finishing, 0);
  M.enqueue(motion, successor, 0);
  M.enqueue(controlMotion, finishing, 0);
  M.enqueue(controlMotion, successor, 0);
  assert.equal(M.stepMotion(controlMotion, 0.2, 0), true);
  motion.onComplete = () => {
    invalidResult = M.stepMotion(motion, NaN, 100);
  };
  const solvesBefore = rig.diagnostics.steps;
  assert.equal(M.stepMotion(motion, 0.2, 0), true);
  assert.equal(invalidResult, false);
  assert.equal(motion.current, successor);
  assert.ok(Math.abs(rig.targets[R.channelIndex("head-yaw")] -
    C.lerp(0.5, -0.5, C.smoothstep(0.1))) < 1e-12);
  assert.equal(motion.completions[0].endedAtMs, 100);
  assert.equal(rig.diagnostics.steps, solvesBefore + 1);
  assert.equal(R.poseHash(rig), R.poseHash(controlRig));
});

test("a nested step beyond outer coverage does not replay the uncovered wall gap", () => {
  const {motion, rig} = fixture("callback-uncovered-gap");
  const {motion:controlMotion, rig:controlRig} = fixture("callback-uncovered-gap");
  const finishing = simplePerformance("gap-finishing", 10, 100, {safeEnd:100});
  const successor = simplePerformance("gap-successor", 10, 1000, {
    targets:{"head-yaw":-0.5},
  });
  let nestedResult = null;
  M.enqueue(motion, finishing, 0);
  M.enqueue(motion, successor, 0);
  M.enqueue(controlMotion, finishing, 0);
  M.enqueue(controlMotion, successor, 0);
  assert.equal(M.stepMotion(controlMotion, 0.21, 0), true);
  motion.onComplete = () => {
    nestedResult = M.stepMotion(motion, 0.01, 250);
  };
  const solvesBefore = rig.diagnostics.steps;
  assert.equal(M.stepMotion(motion, 0.2, 0), true);
  assert.equal(nestedResult, true);
  assert.equal(motion.current, successor);
  assert.ok(Math.abs(rig.targets[R.channelIndex("head-yaw")] -
    C.lerp(0.5, -0.5, C.smoothstep(0.11))) < 1e-12,
  "only the covered outer remainder and nested dt may advance the successor");
  assert.equal(motion.completions[0].endedAtMs, 100);
  assert.equal(rig.diagnostics.steps, solvesBefore + 1);
  assert.equal(R.poseHash(rig), R.poseHash(controlRig),
    "uncovered 200-250ms wall time cannot enter the physical interval union");
});

test("completion callback cannot discard a reentrant higher-priority interrupt", () => {
  const {motion} = fixture("callback-reentrant-interrupt");
  const active = simplePerformance("callback-old", 10, 3000, {safeEnd:100});
  const waiting = urgentPerformance("waiting", 90, 5000);
  const dead = urgentPerformance("dead", 100, 8000);
  M.enqueue(motion, active, 0);
  M.requestInterrupt(motion, waiting, 0);
  motion.onComplete = record => {
    if(record.status === "interrupted"){
      assert.equal(M.requestInterrupt(motion, dead, 100), true);
    }
  };
  M.stepMotion(motion, 0.1, 0);
  assert.equal(motion.current, waiting);
  assert.equal(motion.pendingInterrupt, dead,
    "the callback's higher-priority request must survive the original cut");
});

test("secondary springs cover fur, jaw, abdomen, free wrists, and cable load exactly once", () => {
  const {motion} = fixture("secondary-shape");
  const expectedChildren = [
    ...R.CHANNEL_GROUPS.fur,
    "jaw-open", "belly-compress",
    "front-left-wrist-angle", "front-right-wrist-angle",
    "rear-left-wrist-angle", "rear-right-wrist-angle",
    "cable-load-scale",
  ];
  assert.equal(Object.isFrozen(motion.secondaryLinks), true);
  assert.equal(motion.secondaryLinks.length, expectedChildren.length);
  assert.deepEqual(motion.secondaryLinks.map(link => R.CHANNELS[link.childIndex]),
    expectedChildren);
  assert.equal(new Set(motion.secondaryLinks.map(link => link.childIndex)).size,
    expectedChildren.length);
  for(const link of motion.secondaryLinks){
    assert.ok(Number.isInteger(link.parentIndex) && link.parentIndex >= 0 &&
      link.parentIndex < 120);
    assert.ok(Number.isInteger(link.childIndex) && link.childIndex >= 0 &&
      link.childIndex < 120);
    assert.ok(Number.isFinite(link.follow));
    assert.ok(link.spring && Number.isFinite(link.spring.value) &&
      Number.isFinite(link.spring.velocity));
  }
});

test("fur response is delayed, causal, deterministic, and bounded", () => {
  function run(seed){
    const {motion, rig} = fixture(seed);
    const crest = R.channelIndex("fur-head-crest");
    const head = R.channelIndex("head-yaw");
    const performance = simplePerformance("fur-causal", 10, 4000, {
      targets:{"head-yaw":0.7, "fur-head-crest":0},
    });
    M.enqueue(motion, performance, 0);
    M.stepMotion(motion, 1 / 120, 1000 / 120);
    const first = rig.targets[crest];
    assert.equal(first, 0, "fur cannot anticipate parent acceleration");
    M.stepMotion(motion, 1 / 120, 2000 / 120);
    const second = rig.targets[crest];
    assert.notEqual(second, 0, "fur follows measured parent acceleration one step later");
    for(let frame = 3; frame <= 6000; frame += 1){
      M.stepMotion(motion, 1 / 120, frame * 1000 / 120);
      assert.ok(Number.isFinite(rig.targets[crest]));
      assert.ok(rig.targets[crest] >= R.LIMITS["fur-head-crest"].min &&
        rig.targets[crest] <= R.LIMITS["fur-head-crest"].max);
      assert.ok(Number.isFinite(motion.accelerations[head]));
      if(frame % 240 === 0) assert.deepEqual(R.validatePose(rig), []);
    }
    return {hash:R.poseHash(rig), targets:Array.from(rig.targets), first, second};
  }
  assert.deepEqual(run("fur-repeat"), run("fur-repeat"));
});

test("soft tissue follows parent acceleration and loaded wrists suppress follow-through", () => {
  const {motion, rig} = fixture("secondary-categories");
  for(const link of motion.secondaryLinks){
    const child = R.CHANNELS[link.childIndex];
    if(["jaw-open", "belly-compress", "front-left-wrist-angle",
      "front-right-wrist-angle", "cable-load-scale"].includes(child)){
      const parent = R.CHANNELS[link.parentIndex];
      const limit = R.LIMITS[parent];
      R.setChannelTarget(rig, parent, C.lerp(limit.min, limit.max, 0.7));
    }
  }
  M.stepMotion(motion, 1 / 120, 8);
  M.stepMotion(motion, 1 / 120, 16);
  assert.notEqual(rig.targets[R.channelIndex("jaw-open")], 0);
  assert.notEqual(rig.targets[R.channelIndex("belly-compress")], 0);
  assert.notEqual(rig.targets[R.channelIndex("cable-load-scale")], 1);
  assert.equal(rig.targets[R.channelIndex("front-left-wrist-angle")], 0,
    "loaded wrist remains authored and stable");
  assert.notEqual(rig.targets[R.channelIndex("front-right-wrist-angle")], 0,
    "free wrist receives follow-through");
  assert.deepEqual(R.validatePose(rig), []);
});

test("Rig target rollback resynchronizes active Motion state", () => {
  const {motion, rig} = fixture("motion-target-recovery");
  const yaw = R.channelIndex("head-yaw");
  M.enqueue(motion, simplePerformance("target-recovery", 10, 2000, {
    targets:{"head-yaw":0.7, "fur-head-crest":0.15},
  }), 0);
  assert.equal(M.stepMotion(motion, 0.1, 0), true);
  rig.targets[R.channelIndex("light-key-intensity")] = NaN;
  assert.equal(M.stepMotion(motion, 0.1, 100), false);
  assert.equal(motion.baseTargets[yaw], rig.targets[yaw]);
  for(const buffer of [motion.baseTargets, motion.phaseStarts,
    motion.previousVelocities, motion.accelerations]){
    assert.equal(Array.from(buffer).every(Number.isFinite), true);
  }
  for(const link of motion.secondaryLinks){
    assert.equal(link.spring.value, rig.targets[link.childIndex]);
    assert.equal(link.spring.velocity, 0);
  }
  assert.deepEqual(R.validatePose(rig), []);
  assert.equal(M.stepMotion(motion, 0.1, 200), true);
  for(let nowMs = 300; nowMs < 2000; nowMs += 100){
    assert.equal(M.stepMotion(motion, 0.1, nowMs), true);
  }
  assert.equal(motion.completionCount, 1,
    "physical rollback must not rewind logical phase time");
  assert.equal(motion.completions[0].endedAtMs, 2000);
  assert.deepEqual(R.validatePose(rig), []);
});

test("Rig recovery preserves elapsed brace time and its transition deadline", () => {
  const {motion, rig} = fixture("motion-transition-recovery");
  const active = simplePerformance("transition-active", 10, 1000, {
    primitive:"reach", safeEnd:100, interruptible:false,
    targets:{"front-left-reach-x":6, "front-left-reach-y":-4},
  });
  const waiting = urgentPerformance("waiting", 90, 5000);
  M.enqueue(motion, active, 0);
  M.requestInterrupt(motion, waiting, 0);
  assert.equal(M.stepMotion(motion, 0.1, 0), true);
  assert.equal(motion.transitionPhase, "brace");
  assert.equal(M.stepMotion(motion, 0.08, 100), true);
  rig.targets[R.channelIndex("light-fill-intensity")] = NaN;
  assert.equal(M.stepMotion(motion, 0.04, 180), false);
  assert.equal(motion.transitionPhase, "brace");
  assert.equal(M.stepMotion(motion, 0.04, 220), true);
  assert.equal(motion.transitionPhase, "settle",
    "the recovered brace retains 120ms and ends at the original 260ms boundary");
  assert.equal(motion.completionCount, 1);
  assert.deepEqual(R.validatePose(rig), []);
});

test("Rig velocity rollback removes NaN from causal secondary state", () => {
  const {motion, rig} = fixture("motion-velocity-recovery");
  const head = R.channelIndex("head-yaw");
  M.enqueue(motion, simplePerformance("velocity-recovery", 10, 2000, {
    targets:{"head-yaw":0.7, "fur-head-crest":0.15},
  }), 0);
  assert.equal(M.stepMotion(motion, 0.1, 0), true);
  rig.velocities[head] = NaN;
  assert.equal(M.stepMotion(motion, 0.1, 100), false);
  assert.equal(motion.previousVelocities[head], rig.velocities[head]);
  assert.equal(motion.accelerations[head], 0);
  for(const link of motion.secondaryLinks){
    assert.ok(Number.isFinite(link.spring.value));
    assert.equal(link.spring.velocity, 0);
  }
  assert.deepEqual(R.validatePose(rig), []);
  assert.equal(M.stepMotion(motion, 0.1, 200), true);
  assert.equal(motion.completionCount, 0);
  assert.deepEqual(R.validatePose(rig), []);
});

test("steady stepping preserves allocation shape and consumes no random stream", () => {
  assert.doesNotMatch(MOTION_SOURCE, /Math\.random|createRng|\.next\s*\(/);
  const hotStart = MOTION_SOURCE.indexOf("function processContact");
  const hotEnd = MOTION_SOURCE.indexOf("function stepMotion");
  assert.ok(hotStart >= 0 && hotEnd > hotStart);
  const hotSource = MOTION_SOURCE.slice(hotStart, hotEnd);
  assert.doesNotMatch(hotSource, /\bnew\s+|=\s*\[\s*\]|Object\.(?:keys|values|entries)\s*\(|\.map\s*\(/);
  assert.doesNotMatch(hotSource, /`front-\$\{/,
    "contact stepping must use precomputed limb names");

  const {motion, rig} = fixture("allocation-shape");
  const keys = Reflect.ownKeys(motion);
  const references = {
    owners:motion.owners,
    channelTargets:motion.channelTargets,
    phaseStarts:motion.phaseStarts,
    baseTargets:motion.baseTargets,
    previousVelocities:motion.previousVelocities,
    accelerations:motion.accelerations,
    queueTerminalDeadlines:motion.queueTerminalDeadlines,
    secondaryLinks:motion.secondaryLinks,
    rig:motion.rig,
  };
  M.enqueue(motion, simplePerformance("long-step", 10, 120000, {
    targets:{"head-yaw":0.55, "fur-head-crest":0.15},
  }), 0);
  for(let frame = 1; frame <= 20000; frame += 1){
    assert.equal(M.stepMotion(motion, 1 / 120, frame * 1000 / 120), true);
    if(frame % 1000 === 0) assert.deepEqual(R.validatePose(rig), []);
  }
  assert.deepEqual(Reflect.ownKeys(motion), keys);
  for(const [name, reference] of Object.entries(references)){
    assert.equal(motion[name], reference, `${name} identity changed`);
  }
  assert.equal(Array.from(motion.queueTerminalDeadlines).every(Number.isFinite), true);
  assert.equal(rig.diagnostics.recoveries, 0);
});

test("public operations reject malformed performances and time without corrupting the rig", () => {
  const {motion, rig} = fixture("motion-hostile");
  assert.equal(M.enqueue(motion, null, 0), false);
  assert.equal(M.enqueue(motion, {phases:[]}, 0), false);
  assert.equal(M.requestInterrupt(motion, null, 0), false);
  assert.equal(M.stepMotion(motion, NaN, 0), false);
  assert.equal(M.stepMotion(motion, -1, 0), false);
  assert.equal(M.stepMotion(motion, 0, Infinity), false);
  assert.equal(M.isIdle({}), false);
  assert.deepEqual(R.validatePose(rig), []);
});

test("actual immutable Behavior Phase performances run without schema adaptation", () => {
  const director = B.createDirector("motion-behavior-integration");
  const {motion, rig} = fixture("motion-behavior-integration");
  B.updateContext(director, {sessionId:"integration", status:"waiting", supports:{
    "front-left":{mode:"loaded", load:0.58},
    "rear-right":{mode:"loaded", load:0.42},
  }}, 0);
  const performance = B.nextPerformance(director, 0);
  assert.equal(Object.isFrozen(performance), true);
  assert.equal(Object.isFrozen(performance.phases), true);
  assert.equal(M.enqueue(motion, performance, 0), true);
  for(let frame = 1; frame <= 3000 && !M.isIdle(motion); frame += 1){
    assert.equal(M.stepMotion(motion, 1 / 120, frame * 1000 / 120), true);
    if(frame % 120 === 0) assert.deepEqual(R.validatePose(rig), []);
  }
  assert.equal(M.isIdle(motion), true);
  assert.equal(motion.completions.at(-1).family, performance.family);
  assert.deepEqual(R.validatePose(rig), []);
});
