"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");
require("../../dash/perezos/rig.js");
require("../../dash/perezos/behaviors.js");
require("../../dash/perezos/motion.js");

const NS = global.ComandOSPerezOS;
const R = NS.Rig;
const B = NS.Behaviors;
const M = NS.Motion;

const LIMBS = Object.freeze(["front-left", "front-right", "rear-left", "rear-right"]);
const RARE_COOLDOWNS = Object.freeze({
  scratch:75_000,
  yawn:90_000,
  doze:180_000,
  "slip-recover":120_000,
});
const STATUS_WINDOW_SECONDS = 120;
const CABLE_ENERGY_LIMIT = 25_000;
const INTERRUPT_BRIDGE_MS = 320;
const TRANSITION_STAGE_MS = 160;

function supportContext(rig){
  const supports = {};
  for(const limb of LIMBS){
    const support = rig.supports[limb];
    supports[limb] = {mode:support.mode, load:support.load};
  }
  return supports;
}

function context(seed, status, rig){
  return {
    sessionId:`long-run-${seed}`,
    status,
    role:"daily",
    costume:"bufanda",
    contextPressure:"medium",
    theme:"noche",
    expanded:false,
    supports:supportContext(rig),
  };
}

function failInvariant(label, state, details = {}){
  const channel = details.channel || details.contact || details.owner || "-";
  assert.fail(`${label}: ${JSON.stringify({seed:state.seed,
    timestampMs:state.nowMs, poseHash:R.poseHash(state.rig), channel, ...details})}`);
}

function assertFiniteBoundedStep(state, report){
  const {rig, motion} = state;
  let ownerCount = 0;
  for(let index = 0; index < R.CHANNELS.length; index += 1){
    const name = R.CHANNELS[index];
    const limit = R.LIMITS[name];
    const value = rig.values[index];
    const target = rig.targets[index];
    const velocity = rig.velocities[index];
    if(!Number.isFinite(value) || !Number.isFinite(target) || !Number.isFinite(velocity)){
      report.nonFinite += 1;
      failInvariant("non-finite anatomical channel", state,
        {channel:name, value, target, velocity});
    }
    if(value < limit.min - 1e-9 || value > limit.max + 1e-9 ||
       target < limit.min - 1e-9 || target > limit.max + 1e-9){
      failInvariant("joint/channel escaped its authored limit", state,
        {channel:name, value, target, min:limit.min, max:limit.max});
    }
    const owner = motion.owners[index];
    if(owner !== -1){
      ownerCount += 1;
      const transitionOwner = owner === -2 && motion.transitionPhase !== null;
      const phaseOwner = Number.isInteger(owner) && motion.current && owner >= 0 &&
        owner < motion.current.phases.length;
      if(!transitionOwner && !phaseOwner){
        failInvariant("invalid or stuck channel owner", state, {channel:name, owner});
      }
    }
  }
  if(M.isIdle(motion) && ownerCount !== 0){
    report.stuckOwners += 1;
    failInvariant("idle motion retained channel ownership", state, {owner:ownerCount});
  }

  let supportLoad = 0;
  let loadedSupports = 0;
  for(const limb of LIMBS){
    const support = rig.supports[limb];
    const solved = rig.limbs[limb];
    if(!support || !solved || !Number.isFinite(support.load) ||
       !Number.isFinite(solved.contactError)){
      report.nonFinite += 1;
      failInvariant("non-finite support/contact", state, {contact:limb});
    }
    supportLoad += support.load;
    if(support.mode === "loaded"){
      loadedSupports += 1;
      if(support.load < 0 || support.load > 1 || solved.contactError >= 1){
        report.invalidContacts += 1;
        failInvariant("loaded contact exceeded one logical pixel", state,
          {contact:limb, load:support.load, error:solved.contactError});
      }
    }else if(support.load !== 0){
      report.invalidContacts += 1;
      failInvariant("released support retained load", state,
        {contact:limb, load:support.load});
    }
  }
  if(loadedSupports < 1 || Math.abs(supportLoad - 1) >= 1e-5){
    report.invalidContacts += 1;
    failInvariant("support loads do not form a safe normalized set", state,
      {contact:"support-set", loadedSupports, supportLoad});
  }

  for(let index = 0; index < rig.cable.length; index += 1){
    if(!Number.isFinite(rig.cable[index]) || !Number.isFinite(rig.cablePrevious[index])){
      report.nonFinite += 1;
      failInvariant("non-finite cable node", state,
        {channel:`cable-node-${index}`, value:rig.cable[index],
          previous:rig.cablePrevious[index]});
    }
  }
  const stretch = rig.diagnostics.maxCableStretch;
  const contactError = rig.diagnostics.maxLoadedContactError;
  const energy = rig.diagnostics.cableEnergy;
  report.maxCableStretch = Math.max(report.maxCableStretch, stretch);
  report.maxContactError = Math.max(report.maxContactError, contactError);
  report.maxCableEnergy = Math.max(report.maxCableEnergy, energy);
  if(!Number.isFinite(stretch) || stretch < 0 || stretch >= 0.03){
    failInvariant("cable stretch exceeded the exact solver contract", state,
      {channel:"cable-stretch", stretch});
  }
  if(!Number.isFinite(contactError) || contactError < 0 || contactError >= 1){
    report.invalidContacts += 1;
    failInvariant("contact diagnostic escaped its exact bound", state,
      {contact:"loaded-contact", contactError});
  }
  if(!Number.isFinite(energy) || energy < 0 || energy > CABLE_ENERGY_LIMIT){
    failInvariant("cable energy became unbounded", state,
      {channel:"cable-energy", energy, limit:CABLE_ENERGY_LIMIT});
  }
  if(rig.diagnostics.recoveries !== 0){
    failInvariant("valid long-run input required solver recovery", state,
      {channel:"solver-recoveries", recoveries:rig.diagnostics.recoveries});
  }
}

function trackOwnerAges(state, report){
  const {motion} = state;
  const frameToleranceMs = 1000 / state.hz;
  const transitionCode = motion.transitionPhase === "brace" ? 1 :
    motion.transitionPhase === "settle" ? 2 : 0;
  for(let index = 0; index < motion.owners.length; index += 1){
    const owner = motion.owners[index];
    if(owner === -1){
      state.ownerValues[index] = -1;
      state.ownerPerformances[index] = null;
      state.ownerTransitions[index] = 0;
      continue;
    }
    const sameOwner = state.ownerValues[index] === owner &&
      state.ownerPerformances[index] === motion.current &&
      state.ownerTransitions[index] === transitionCode;
    if(!sameOwner){
      state.ownerValues[index] = owner;
      state.ownerPerformances[index] = motion.current;
      state.ownerTransitions[index] = transitionCode;
      state.ownerStartedMs[index] = state.nowMs;
    }
    const ageMs = state.nowMs - state.ownerStartedMs[index];
    if(ageMs > report.maxOwnerAgeMs) report.maxOwnerAgeMs = ageMs;
    const phase = owner === -2 ? null : motion.current && motion.current.phases[owner];
    const declaredMs = owner === -2 ? TRANSITION_STAGE_MS : phase && phase.durationMs;
    const allowanceMs = owner === -2 ? frameToleranceMs :
      INTERRUPT_BRIDGE_MS + frameToleranceMs;
    if(!Number.isFinite(declaredMs) || ageMs > declaredMs + allowanceMs + 1e-6){
      report.stuckOwners += 1;
      failInvariant("channel owner exceeded declared lifetime", state, {
        channel:R.CHANNELS[index], owner, ageMs, declaredMs, allowanceMs,
        phase:owner === -2 ? motion.transitionPhase : phase && phase.primitive,
      });
    }
  }
}

function recordPerformance(state, performance, report){
  const signature = B.performanceSignature(performance);
  report.allSignatures.add(signature);
  if(performance.state === "idle") report.idleSignatures.add(signature);
  report.behaviorFamilies.add(performance.family);
  for(const phase of performance.phases) report.primitiveFamilies.add(phase.primitive);
  const cooldown = RARE_COOLDOWNS[performance.family] || 0;
  if(!cooldown) return;
  const previous = state.rareStarts[performance.family];
  if(previous !== undefined && performance.createdAtMs < previous + cooldown){
    report.cooldownViolations += 1;
    failInvariant("rare behavior repeated inside its cooldown", state,
      {channel:performance.family, previous, createdAtMs:performance.createdAtMs, cooldown});
  }
  if(state.director.cooldowns[performance.family] !== performance.createdAtMs + cooldown){
    report.cooldownViolations += 1;
    failInvariant("rare cooldown reservation is not exact", state,
      {channel:performance.family, reservation:state.director.cooldowns[performance.family],
        expected:performance.createdAtMs + cooldown});
  }
  state.rareStarts[performance.family] = performance.createdAtMs;
}

function handoff(state, report){
  B.updateContext(state.director, context(state.seed, state.status, state.rig), state.nowMs);
  const performance = B.nextPerformance(state.director, state.nowMs);
  if(performance !== state.activePerformance){
    state.activePerformance = performance;
    recordPerformance(state, performance, report);
  }
  if(M.isIdle(state.motion)) return M.enqueue(state.motion, performance, state.nowMs);
  if(state.motion.current === performance || state.motion.pendingInterrupt === performance){
    return true;
  }
  return M.requestInterrupt(state.motion, performance, state.nowMs);
}

function checkDeadline(state, report){
  const deadline = state.deadline;
  if(!deadline) return;
  if(deadline.status === "waiting" && state.motion.current &&
     state.motion.current.state === "waiting"){
    const elapsed = state.nowMs - deadline.startedAtMs;
    if(elapsed > 5_000 + 1e-6){
      report.deadlineMisses += 1;
      failInvariant("waiting acknowledgement missed five-second deadline", state,
        {channel:"waiting", elapsed});
    }
    state.deadline = null;
    return;
  }
  if(deadline.status === "dead"){
    const completion = state.motion.completions.at(-1);
    if(completion && completion.state === "dead" &&
       completion.endedAtMs >= deadline.startedAtMs){
      const elapsed = completion.endedAtMs - deadline.startedAtMs;
      if(elapsed > 8_000 + 1e-6){
        report.deadlineMisses += 1;
        failInvariant("dead safe pose missed eight-second deadline", state,
          {channel:"dead", elapsed, completion});
      }
      state.deadline = null;
      return;
    }
  }
  const limit = deadline.status === "waiting" ? 5_000 : 8_000;
  if(state.nowMs - deadline.startedAtMs > limit + 1000 / state.hz){
    report.deadlineMisses += 1;
    failInvariant(`${deadline.status} response deadline expired`, state,
      {channel:deadline.status, elapsed:state.nowMs - deadline.startedAtMs});
  }
}

function signatureSweep(seeds, report){
  for(const seed of seeds){
    const director = B.createDirector(`signature-${seed}`);
    const rig = R.createRig(`signature-${seed}`);
    for(let index = 0; index < 1_600; index += 1){
      const nowMs = index * 240_001;
      B.updateContext(director, context(`signature-${seed}`, "idle", rig), nowMs);
      const performance = B.nextPerformance(director, nowMs);
      const signature = B.performanceSignature(performance);
      report.idleSignatures.add(signature);
      report.allSignatures.add(signature);
      report.behaviorFamilies.add(performance.family);
      for(const phase of performance.phases) report.primitiveFamilies.add(phase.primitive);
      assert.equal(B.completePerformance(director, performance,
        nowMs + performance.durationMs), true);
    }
  }
}

function simulate({seeds, statuses, seconds, hz}){
  assert.ok(Array.isArray(seeds) && seeds.length >= 2);
  assert.deepEqual(statuses, ["idle", "working", "waiting", "done", "dead"]);
  assert.equal(seconds, 21_600);
  assert.equal(hz, 30);
  const report = {
    nonFinite:0,
    invalidContacts:0,
    deadlineMisses:0,
    cooldownViolations:0,
    stuckOwners:0,
    maxOwnerAgeMs:0,
    maxCableStretch:0,
    maxContactError:0,
    maxCableEnergy:0,
    statusFrames:Object.fromEntries(statuses.map(status => [status, 0])),
    idleSignatures:new Set(),
    allSignatures:new Set(),
    behaviorFamilies:new Set(),
    primitiveFamilies:new Set(),
  };
  signatureSweep(seeds, report);
  const frames = seconds * hz;
  const statusWindowFrames = STATUS_WINDOW_SECONDS * hz;
  const dt = 1 / hz;

  for(let seedIndex = 0; seedIndex < seeds.length; seedIndex += 1){
    const seed = seeds[seedIndex];
    const rig = R.createRig(`long-run-${seed}`);
    const director = B.createDirector(`long-run-${seed}`);
    const motion = M.createMotion(rig);
    const ownerValues = new Int16Array(R.CHANNELS.length);
    ownerValues.fill(-32768);
    const state = {seed, rig, director, motion, activePerformance:null,
      rareStarts:Object.create(null), nowMs:0, status:"", deadline:null, hz,
      ownerValues, ownerPerformances:new Array(R.CHANNELS.length).fill(null),
      ownerTransitions:new Int8Array(R.CHANNELS.length),
      ownerStartedMs:new Float64Array(R.CHANNELS.length)};
    motion.onComplete = completion => {
      if(completion.status === "completed" && state.activePerformance &&
         completion.family === state.activePerformance.family){
        B.completePerformance(director, state.activePerformance, completion.endedAtMs);
        state.activePerformance = null;
      }
    };

    for(let frame = 0; frame < frames; frame += 1){
      state.nowMs = frame * 1000 / hz;
      const statusIndex = (Math.floor(frame / statusWindowFrames) + seedIndex) % statuses.length;
      const status = statuses[statusIndex];
      report.statusFrames[status] += 1;
      if(status !== state.status){
        if(state.deadline){
          report.deadlineMisses += 1;
          failInvariant("status window ended before urgent response", state,
            {channel:state.deadline.status});
        }
        state.status = status;
        if(status === "waiting" || status === "dead"){
          state.deadline = {status, startedAtMs:state.nowMs};
        }
        handoff(state, report);
      }else if(M.isIdle(motion)){
        handoff(state, report);
      }

      if(!M.stepMotion(motion, dt, state.nowMs)){
        failInvariant("motion/rig solver rejected a valid scheduled step", state,
          {channel:motion.phase ? motion.phase.primitive : "idle"});
      }
      assertFiniteBoundedStep(state, report);
      trackOwnerAges(state, report);
      checkDeadline(state, report);
      if(frame % (hz * 60) === 0){
        const errors = R.validatePose(rig);
        if(errors.length) failInvariant("minute topology audit failed", state,
          {channel:errors[0], errors});
      }
    }
    if(state.deadline){
      report.deadlineMisses += 1;
      failInvariant("simulation ended before urgent response", state,
        {channel:state.deadline.status});
    }
  }

  return Object.freeze({
    nonFinite:report.nonFinite,
    invalidContacts:report.invalidContacts,
    deadlineMisses:report.deadlineMisses,
    cooldownViolations:report.cooldownViolations,
    stuckOwners:report.stuckOwners,
    maxOwnerAgeMs:report.maxOwnerAgeMs,
    maxCableStretch:report.maxCableStretch,
    maxContactError:report.maxContactError,
    maxCableEnergy:report.maxCableEnergy,
    statusFrames:Object.freeze({...report.statusFrames}),
    idleSignatures:report.idleSignatures.size,
    allSignatures:report.allSignatures.size,
    behaviorFamilies:Object.freeze([...report.behaviorFamilies].sort()),
    primitiveFamilies:Object.freeze([...report.primitiveFamilies].sort()),
    missingPrimitiveFamilies:Object.freeze(Object.keys(B.PRIMITIVES)
      .filter(primitive => !report.primitiveFamilies.has(primitive))),
  });
}

function runFocused(seed, finalFrame, statusAtFrame){
  const hz = 30;
  const rig = R.createRig(`long-run-${seed}`);
  const director = B.createDirector(`long-run-${seed}`);
  const motion = M.createMotion(rig);
  const report = {idleSignatures:new Set(), allSignatures:new Set(),
    behaviorFamilies:new Set(), primitiveFamilies:new Set(), cooldownViolations:0,
    stuckOwners:0, maxOwnerAgeMs:0};
  const ownerValues = new Int16Array(R.CHANNELS.length);
  ownerValues.fill(-32768);
  const state = {seed, rig, director, motion, activePerformance:null,
    rareStarts:Object.create(null), nowMs:0, status:"", deadline:null, hz,
    ownerValues, ownerPerformances:new Array(R.CHANNELS.length).fill(null),
    ownerTransitions:new Int8Array(R.CHANNELS.length),
    ownerStartedMs:new Float64Array(R.CHANNELS.length)};
  motion.onComplete = completion => {
    if(completion.status === "completed" && state.activePerformance &&
       completion.family === state.activePerformance.family){
      B.completePerformance(director, state.activePerformance, completion.endedAtMs);
      state.activePerformance = null;
    }
  };
  for(let frame = 0; frame <= finalFrame; frame += 1){
    state.nowMs = frame * 1000 / hz;
    const status = statusAtFrame(frame);
    if(status !== state.status){
      state.status = status;
      handoff(state, report);
    }else if(M.isIdle(motion)) handoff(state, report);
    assert.equal(M.stepMotion(motion, 1 / hz, state.nowMs), true,
      `seed=${seed} timestampMs=${state.nowMs} poseHash=${R.poseHash(rig)} ` +
      `channel=${motion.phase ? motion.phase.primitive : "idle"}`);
    trackOwnerAges(state, report);
  }
  assert.equal(rig.diagnostics.recoveries, 0);
  return rig;
}

test("channel-owner age is continuous and bounded by phase plus interruption bridges", () => {
  const rig = R.createRig("owner-age-watchdog");
  const motion = {owners:new Int16Array(R.CHANNELS.length), phaseIndex:0,
    phase:{primitive:"breathe", durationMs:100}, transitionPhase:null,
    current:{family:"owner-probe", createdAtMs:0, phases:[{durationMs:100}]},
    pendingInterrupt:null};
  motion.owners.fill(-1);
  motion.owners[0] = 0;
  const state = {seed:"owner-age-watchdog", rig, motion, nowMs:0, hz:30,
    ownerValues:new Int16Array(R.CHANNELS.length),
    ownerPerformances:new Array(R.CHANNELS.length).fill(null),
    ownerTransitions:new Int8Array(R.CHANNELS.length),
    ownerStartedMs:new Float64Array(R.CHANNELS.length)};
  state.ownerValues.fill(-32768);
  const report = {stuckOwners:0, maxOwnerAgeMs:0};
  trackOwnerAges(state, report);
  state.nowMs = 453;
  trackOwnerAges(state, report);
  assert.equal(report.maxOwnerAgeMs, 453);
  state.nowMs = 454;
  assert.throws(() => trackOwnerAges(state, report), /channel owner exceeded declared lifetime/);
  motion.phaseIndex = 1;
  motion.phase = {primitive:"neutral", durationMs:350};
  motion.current.phases.push(motion.phase);
  motion.owners[0] = 1;
  state.nowMs = 455;
  trackOwnerAges(state, report);
  assert.equal(state.ownerStartedMs[0], 455, "phase identity change resets continuous age");
});

test("working seed 7 does not recover while shift-weight crosses 24 seconds", () => {
  runFocused(7, 724, () => "working");
});

test("seed 7 idle recovery remains valid after repeated status transitions", () => {
  const statuses = ["idle", "working", "waiting", "done", "dead"];
  runFocused(7, 35_525, frame =>
    statuses[(Math.floor(frame / (STATUS_WINDOW_SECONDS * 30)) + 1) % statuses.length]);
});

test("six visible hours remain finite, supported, diverse, and bounded", {timeout:0}, () => {
  const report = simulate({seeds:[1, 7, 19, 41, 97, 193, 389, 769],
    statuses:["idle", "working", "waiting", "done", "dead"],
    seconds:21_600, hz:30});
  assert.equal(report.nonFinite, 0);
  assert.equal(report.invalidContacts, 0);
  assert.equal(report.deadlineMisses, 0);
  assert.equal(report.cooldownViolations, 0);
  assert.equal(report.stuckOwners, 0);
  assert.ok(report.maxCableStretch < 0.03);
  assert.ok(report.maxContactError < 1);
  assert.ok(report.maxCableEnergy <= CABLE_ENERGY_LIMIT);
  assert.ok(report.idleSignatures >= 10_000, `${report.idleSignatures} idle signatures`);
  assert.deepEqual(report.missingPrimitiveFamilies, []);
  for(const frames of Object.values(report.statusFrames)) assert.ok(frames > 0);
  process.stdout.write(`\nPEREZOS_LONG_RUN ${JSON.stringify(report)}\n`);
});

module.exports = {simulate};
