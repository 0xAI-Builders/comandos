"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");

global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");
require("../../dash/perezos/rig.js");

const A = global.ComandOSPerezOS.Art;
const R = global.ComandOSPerezOS.Rig;
const RIG_SOURCE = fs.readFileSync(require.resolve("../../dash/perezos/rig.js"), "utf8");

const ART_CHAINS = Object.freeze({
  "front-left":Object.freeze(["arm-fl-upper", "arm-fl-fore", "palm-fl"]),
  "front-right":Object.freeze(["arm-fr-upper", "arm-fr-fore", "palm-fr"]),
  "rear-left":Object.freeze(["leg-rl-upper", "leg-rl-lower", "palm-rl"]),
  "rear-right":Object.freeze(["leg-rr-upper", "leg-rr-lower", "palm-rr"]),
});

function artAnchor(id){
  const part = A.PARTS.find(candidate => candidate.id === id);
  return {x:part.bounds[0] + part.pivot[0], y:part.bounds[1] + part.pivot[1]};
}

function expectedChain(ids){
  const [root, joint, end] = ids.map(artAnchor);
  const upperLength = Math.hypot(joint.x - root.x, joint.y - root.y);
  const lowerLength = Math.hypot(end.x - joint.x, end.y - joint.y);
  const cross = (joint.x - root.x) * (end.y - root.y) -
    (joint.y - root.y) * (end.x - root.x);
  return {root, joint, end, upperLength, lowerLength, bend:-Math.sign(cross)};
}

function assertPointNear(actual, expected, tolerance = 1e-9){
  assert.ok(Math.abs(actual.x - expected.x) <= tolerance,
    `x ${actual.x} differs from ${expected.x}`);
  assert.ok(Math.abs(actual.y - expected.y) <= tolerance,
    `y ${actual.y} differs from ${expected.y}`);
}

function normalizedBranchCross(root, joint, point){
  const upperX = joint.x - root.x;
  const upperY = joint.y - root.y;
  const pointX = point.x - root.x;
  const pointY = point.y - root.y;
  return (upperX * pointY - upperY * pointX) /
    (Math.hypot(upperX, upperY) * Math.hypot(pointX, pointY));
}

function reflectAcrossRootEnd(joint, root, end){
  const lineX = end.x - root.x;
  const lineY = end.y - root.y;
  const projection = ((joint.x - root.x) * lineX + (joint.y - root.y) * lineY) /
    (lineX * lineX + lineY * lineY);
  const projectedX = root.x + projection * lineX;
  const projectedY = root.y + projection * lineY;
  return {x:2 * projectedX - joint.x, y:2 * projectedY - joint.y};
}

function sampleCable(rig, t, bias){
  const bounded = Math.max(0, Math.min(1, t + bias * 0.02));
  const scaled = bounded * 8;
  const node = Math.min(7, Math.floor(scaled));
  const fraction = scaled - node;
  const offset = node * 2;
  return {
    x:rig.cable[offset] + (rig.cable[offset + 2] - rig.cable[offset]) * fraction,
    y:rig.cable[offset + 1] + (rig.cable[offset + 3] - rig.cable[offset + 1]) * fraction,
  };
}

function isReachable(chain, root, point){
  const distance = Math.hypot(point.x - root.x, point.y - root.y);
  return distance > Math.abs(chain.upperLength - chain.lowerLength) + 0.001 &&
    distance < chain.upperLength + chain.lowerLength - 0.001;
}

const EXPECTED_GROUPS = Object.freeze({
  axial:Object.freeze([
    "spine-pelvis-x", "spine-pelvis-y", "spine-pelvis-angle",
    "spine-lower-angle", "spine-mid-angle", "spine-upper-angle",
    "neck-lower-angle", "neck-mid-angle", "neck-upper-angle",
    "head-yaw", "head-pitch", "head-roll", "chest-expand",
    "belly-compress", "body-lean-x", "body-lift",
  ]),
  face:Object.freeze([
    "jaw-open", "muzzle-lift", "nose-twitch", "face-turn",
    "eye-left-look-x", "eye-left-look-y", "eye-right-look-x", "eye-right-look-y",
    "lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
    "brow-left-lift", "brow-right-lift", "cheek-left-puff", "cheek-right-puff",
  ]),
  limbs:Object.freeze([
    "front-left-shoulder-angle", "front-left-elbow-angle", "front-left-wrist-angle",
    "front-left-palm-angle", "front-left-reach-x", "front-left-reach-y",
    "front-left-lift", "front-left-twist",
    "front-right-shoulder-angle", "front-right-elbow-angle", "front-right-wrist-angle",
    "front-right-palm-angle", "front-right-reach-x", "front-right-reach-y",
    "front-right-lift", "front-right-twist",
    "rear-left-shoulder-angle", "rear-left-elbow-angle", "rear-left-wrist-angle",
    "rear-left-palm-angle", "rear-left-reach-x", "rear-left-reach-y",
    "rear-left-lift", "rear-left-twist",
    "rear-right-shoulder-angle", "rear-right-elbow-angle", "rear-right-wrist-angle",
    "rear-right-palm-angle", "rear-right-reach-x", "rear-right-reach-y",
    "rear-right-lift", "rear-right-twist",
  ]),
  claws:Object.freeze([
    "claw-front-left-1-curl", "claw-front-left-1-spread",
    "claw-front-left-2-curl", "claw-front-left-2-spread",
    "claw-front-left-3-curl", "claw-front-left-3-spread",
    "claw-front-right-1-curl", "claw-front-right-1-spread",
    "claw-front-right-2-curl", "claw-front-right-2-spread",
    "claw-front-right-3-curl", "claw-front-right-3-spread",
    "claw-rear-left-1-curl", "claw-rear-left-1-spread",
    "claw-rear-left-2-curl", "claw-rear-left-2-spread",
    "claw-rear-left-3-curl", "claw-rear-left-3-spread",
    "claw-rear-right-1-curl", "claw-rear-right-1-spread",
    "claw-rear-right-2-curl", "claw-rear-right-2-spread",
    "claw-rear-right-3-curl", "claw-rear-right-3-spread",
  ]),
  fur:Object.freeze([
    "fur-head-crest", "fur-head-cheek-left", "fur-head-cheek-right", "fur-head-nape",
    "fur-neck-ruff-left", "fur-neck-ruff-right", "fur-back-shoulder", "fur-back-mid",
    "fur-back-rump", "fur-belly-chest", "fur-belly-mid", "fur-belly-flank",
  ]),
  cable:Object.freeze([
    "cable-sway", "cable-lift", "cable-tension", "cable-damping",
    "cable-wind", "cable-pulse", "cable-contact-bias", "cable-load-scale",
  ]),
  light:Object.freeze([
    "light-key-intensity", "light-fill-intensity", "light-rim-intensity",
    "light-loaded-pulse", "light-searching-pulse", "light-visor-glow",
  ]),
  props:Object.freeze([
    "prop-corona-tilt", "prop-casco-tilt", "prop-visor-open",
    "prop-fuego-height", "prop-bufanda-sway", "prop-huevo-wobble",
  ]),
});

const snapshotSupports = rig => Object.fromEntries(Object.entries(rig.supports).map(
  ([name, support]) => [name, {...support, point:{...support.point}, normal:{...support.normal}}]));

const snapshotBuffers = rig => ({
  values:Array.from(rig.values),
  targets:Array.from(rig.targets),
  velocities:Array.from(rig.velocities),
  lastValidValues:Array.from(rig.lastValidValues),
  lastValidTargets:Array.from(rig.lastValidTargets),
  lastValidVelocities:Array.from(rig.lastValidVelocities),
  cable:Array.from(rig.cable),
  cablePrevious:Array.from(rig.cablePrevious),
  cableRestLengths:Array.from(rig.cableRestLengths),
  lastValidCable:Array.from(rig.lastValidCable),
  lastValidCablePrevious:Array.from(rig.lastValidCablePrevious),
  lastValidSupports:Array.from(rig.lastValidSupports),
  lastValidLimbs:Array.from(rig.lastValidLimbs),
  lastValidClaws:Array.from(rig.lastValidClaws),
});

const snapshotCanonical = rig => ({
  ...snapshotBuffers(rig),
  supports:snapshotSupports(rig),
  limbs:JSON.parse(JSON.stringify(rig.limbs)),
  claws:JSON.parse(JSON.stringify(rig.claws)),
  transferGoal:rig.transferGoal,
  lastValidTransferGoal:rig.lastValidTransferGoal,
  lastDt:rig.lastDt,
  lastValidDt:rig.lastValidDt,
  diagnostics:{
    steps:rig.diagnostics.steps,
    maxCableStretch:rig.diagnostics.maxCableStretch,
    maxLoadedContactError:rig.diagnostics.maxLoadedContactError,
    cableEnergy:rig.diagnostics.cableEnergy,
  },
  lastValidSteps:rig.lastValidSteps,
  lastValidMaxCableStretch:rig.lastValidMaxCableStretch,
  lastValidMaxLoadedContactError:rig.lastValidMaxLoadedContactError,
  lastValidCableEnergy:rig.lastValidCableEnergy,
  cableIterations:rig.cableIterations,
});

test("rig exposes 120 finite channels and twelve claw grips", () => {
  const rig = R.createRig(42);
  assert.equal(R.CHANNELS.length, 120);
  assert.equal(rig.values.length, 120);
  assert.equal(Object.keys(rig.claws).length, 12);
  assert.deepEqual(R.validatePose(rig), []);
});

test("channel groups, names, and immutable limits are exact", () => {
  assert.deepEqual(R.CHANNEL_GROUPS, EXPECTED_GROUPS);
  assert.deepEqual(R.CHANNELS, Object.values(EXPECTED_GROUPS).flat());
  assert.equal(new Set(R.CHANNELS).size, 120);
  assert.equal(Object.isFrozen(R.CHANNELS), true);
  assert.equal(Object.isFrozen(R.CHANNEL_GROUPS), true);
  assert.equal(Object.isFrozen(R.LIMITS), true);

  for(const [group, names] of Object.entries(R.CHANNEL_GROUPS)){
    assert.equal(Object.isFrozen(names), true, `${group} group must be frozen`);
    for(const name of names){
      const limit = R.LIMITS[name];
      assert.ok(limit, `${name} needs a limit`);
      assert.equal(Object.isFrozen(limit), true, `${name} limit must be frozen`);
      assert.ok(Number.isFinite(limit.min), `${name} min`);
      assert.ok(Number.isFinite(limit.max), `${name} max`);
      assert.ok(Number.isFinite(limit.default), `${name} default`);
      assert.ok(limit.min < limit.max, `${name} ordered bounds`);
      assert.ok(limit.default >= limit.min && limit.default <= limit.max, `${name} default bounds`);
    }
  }
  assert.deepEqual(Object.keys(R.LIMITS), R.CHANNELS);
});

test("rig preallocates typed live, target, cable, and rollback buffers", () => {
  const rig = R.createRig(42);
  for(const name of ["values", "targets", "velocities", "lastValidValues",
    "lastValidTargets", "lastValidVelocities", "cable", "cablePrevious",
    "cableRestLengths", "lastValidCable", "lastValidCablePrevious",
    "lastValidSupports", "lastValidLimbs", "lastValidClaws"]){
    assert.ok(rig[name] instanceof Float64Array, `${name} must be Float64Array`);
    assert.equal(Object.isSealed(rig[name]), true, `${name} must reject method shadowing`);
  }
  assert.equal(Reflect.defineProperty(rig.values, "set", {value(){ throw new Error("hostile"); }}),
    false);
  assert.equal(Reflect.defineProperty(rig.cable, Symbol("extra"), {value:true}), false);
  assert.equal(rig.values.length, 120);
  assert.equal(rig.targets.length, 120);
  assert.equal(rig.lastValidValues.length, 120);
  assert.equal(rig.cable.length, 18);
  assert.equal(rig.cablePrevious.length, 18);
  assert.equal(rig.lastValidCable.length, 18);
  assert.equal(rig.lastValidCablePrevious.length, 18);
  assert.equal(rig.cableRestLengths.length, 8);
  assert.equal(rig.cableIterations, 8);
});

test("rig construction does not seal populated typed arrays in Chrome-incompatible form", () => {
  const nativeSeal = Object.seal;
  Object.seal = value => {
    if(ArrayBuffer.isView(value) && value.byteLength > 0){
      throw new TypeError("Cannot seal array buffer views with elements");
    }
    return nativeSeal(value);
  };
  try{
    let rig;
    assert.doesNotThrow(() => { rig = R.createRig("chrome-typed-array-contract"); });
    assert.equal(Object.isExtensible(rig.values), false);
    assert.equal(Reflect.defineProperty(rig.values, "set", {value(){}}), false);
    rig.values[0] = 3;
    assert.equal(rig.values[0], 3, "physics elements must remain writable");
  }finally{
    Object.seal = nativeSeal;
  }
});

test("initial support is the authored safe diagonal", () => {
  const rig = R.createRig(42);
  assert.deepEqual(rig.supports["front-left"], {
    limb:"front-left", mode:"loaded", cableT:0.34, load:0.58,
    point:{x:76, y:35}, normal:{x:0, y:-1},
  });
  assert.equal(rig.supports["rear-right"].mode, "loaded");
  assert.equal(rig.supports["rear-right"].load, 0.42);
  assert.equal(rig.supports["front-right"].mode, "released");
  assert.equal(rig.supports["rear-left"].mode, "released");
  assert.deepEqual(rig.supports["rear-right"].point, {x:150, y:86.72222222222223});
  assert.ok(Math.abs(Object.values(rig.supports).reduce((sum, support) =>
    sum + support.load, 0) - 1) < 1e-12);
});

test("loaded grip cannot release without safe transferred support", () => {
  const rig = R.createRig(42);
  assert.equal(R.requestGrip(rig, "front-left", "release", 0.4), false);
  assert.equal(R.requestGrip(rig, "rear-right", "loaded", 0.72), true);
  for(let i = 0; i < 240; i += 1) R.solveRig(rig, 1 / 120);
  assert.equal(R.requestGrip(rig, "front-left", "release", 0.4), true);
  assert.ok(Math.abs(Object.values(rig.supports).reduce((sum, support) =>
    sum + support.load, 0) - 1) < 1e-5);
});

test("release requires one other loaded support at the safety threshold", () => {
  const split = R.createRig(43);
  assert.equal(R.requestGrip(split, "front-right", "loaded", 0.72), true);
  split.supports["front-left"].load = 0;
  split.supports["front-right"].load = 0.5;
  split.supports["rear-right"].load = 0.5;
  split.transferGoal = -1;
  assert.deepEqual(R.validatePose(split), []);
  const before = snapshotSupports(split);
  assert.equal(R.requestGrip(split, "front-left", "release", 0.34), false);
  assert.deepEqual(snapshotSupports(split), before);
  assert.equal(split.diagnostics.recoveries, 0);

  const threshold = R.createRig(44);
  threshold.supports["front-left"].load = 0.05;
  threshold.supports["rear-right"].load = 0.95;
  assert.equal(R.requestGrip(threshold, "front-left", "release", 0.34), true);
  assert.equal(threshold.supports["front-left"].load, 0);
  assert.equal(threshold.supports["rear-right"].load, 1);
  assert.deepEqual(R.validatePose(threshold), []);
});

test("released loads and poses with no loaded support roll back atomically", () => {
  const releasedLoad = R.createRig(45);
  const releasedBefore = snapshotSupports(releasedLoad);
  releasedLoad.supports["front-left"].load -= 0.1;
  releasedLoad.supports["front-right"].load = 0.1;
  assert.equal(R.solveRig(releasedLoad, 1 / 60), false);
  assert.deepEqual(snapshotSupports(releasedLoad), releasedBefore);
  assert.equal(releasedLoad.diagnostics.recoveries, 1);

  const unsupported = R.createRig(46);
  const unsupportedBefore = snapshotSupports(unsupported);
  for(const support of Object.values(unsupported.supports)) support.mode = "released";
  for(const claw of Object.values(unsupported.claws)) claw.mode = "released";
  assert.equal(R.solveRig(unsupported, 1 / 60), false);
  assert.deepEqual(snapshotSupports(unsupported), unsupportedBefore);
  assert.equal(unsupported.diagnostics.recoveries, 1);
});

test("grip requests reject invalid limbs, modes, positions, and unsafe final release", () => {
  const rig = R.createRig(9);
  assert.equal(R.requestGrip(rig, "tail", "loaded", 0.5), false);
  assert.equal(R.requestGrip(rig, "front-right", "hovering", 0.5), false);
  assert.equal(R.requestGrip(rig, "front-right", "loaded", NaN), false);
  assert.equal(R.requestGrip(rig, "front-right", "loaded", Infinity), false);

  R.requestGrip(rig, "rear-right", "loaded", 0.72);
  for(let i = 0; i < 240; i += 1) R.solveRig(rig, 1 / 120);
  assert.equal(R.requestGrip(rig, "front-left", "release", 0.34), true);
  assert.equal(R.requestGrip(rig, "rear-right", "release", 0.72), false);
  assert.deepEqual(R.validatePose(rig), []);

  const corruptTransfer = R.createRig(91);
  corruptTransfer.transferGoal = 99;
  assert.doesNotThrow(() => {
    assert.equal(R.requestGrip(corruptTransfer, "front-left", "loaded", 0.34), false);
  });
  assert.equal(corruptTransfer.diagnostics.recoveries, 1);
  assert.deepEqual(R.validatePose(corruptTransfer), []);
});

test("grip reach checks use the same min/max biased cable sample as solving", () => {
  const biasIndex = R.channelIndex("cable-contact-bias");
  const chain = expectedChain(ART_CHAINS["front-left"]);
  for(const bias of [R.LIMITS["cable-contact-bias"].min,
                      R.LIMITS["cable-contact-bias"].max]){
    const rig = R.createRig(`bias-${bias}`);
    assert.equal(R.setChannelTarget(rig, "cable-contact-bias", bias), true);
    for(let frame = 0; frame < 600; frame += 1){
      assert.equal(R.solveRig(rig, 1 / 120), true);
    }
    assert.ok(Math.abs(rig.values[biasIndex] - bias) < 1e-9);

    let rejectedT = null;
    for(let step = 1; step < 10000; step += 1){
      const t = step / 10000;
      const unbiased = sampleCable(rig, t, 0);
      const biased = sampleCable(rig, t, rig.values[biasIndex]);
      if(rejectedT === null && isReachable(chain, rig.limbs["front-left"].root, unbiased) &&
         !isReachable(chain, rig.limbs["front-left"].root, biased)) rejectedT = t;
    }
    assert.notEqual(rejectedT, null, `no rejected boundary for bias ${bias}`);
    assert.equal(R.requestGrip(rig, "front-left", "loaded", rejectedT), false);
    assert.equal(R.requestGrip(rig, "front-left", "loaded", 0.34), true);
    assert.equal(R.solveRig(rig, 1 / 120), true);
    assert.equal(rig.diagnostics.recoveries, 0);
    assert.ok(rig.limbs["front-left"].contactError < 1);
  }
});

test("finite targets and dt are bounded while zero dt is stable", () => {
  const rig = R.createRig(13);
  assert.equal(R.setChannelTarget(rig, "spine-mid-angle", 1e300), true);
  assert.equal(R.solveRig(rig, 1e300), true);
  assert.equal(rig.targets[R.channelIndex("spine-mid-angle")], R.LIMITS["spine-mid-angle"].max);
  assert.deepEqual(R.validatePose(rig), []);

  const before = R.poseHash(rig);
  assert.equal(R.solveRig(rig, 0), true);
  assert.equal(R.poseHash(rig), before);
  assert.equal(rig.diagnostics.recoveries, 0);
});

test("zero dt clamps finite target packets without advancing live dynamics", () => {
  const rig = R.createRig(14);
  const valuesBefore = Array.from(rig.values);
  const cableBefore = Array.from(rig.cable);
  const previousBefore = Array.from(rig.cablePrevious);
  assert.equal(R.setChannelTarget(rig, "head-yaw", -1e300), true);
  assert.equal(R.solveRig(rig, 0), true);
  assert.equal(rig.targets[R.channelIndex("head-yaw")], R.LIMITS["head-yaw"].min);
  assert.deepEqual(Array.from(rig.values), valuesBefore);
  assert.deepEqual(Array.from(rig.cable), cableBefore);
  assert.deepEqual(Array.from(rig.cablePrevious), previousBefore);
  assert.equal(rig.diagnostics.recoveries, 0);
});

test("solver recovers from invalid targets to its last valid pose", () => {
  const rig = R.createRig(7);
  const before = R.poseHash(rig);
  R.setChannelTarget(rig, "spine-mid-angle", Number.POSITIVE_INFINITY);
  assert.equal(R.solveRig(rig, 1 / 30), false);
  assert.deepEqual(R.validatePose(rig), []);
  assert.equal(rig.diagnostics.recoveries, 1);
  assert.equal(R.poseHash(rig), before);
});

test("NaN and Infinity failures atomically restore every state buffer and support", () => {
  const rig = R.createRig(17);
  for(let i = 0; i < 60; i += 1) R.solveRig(rig, 1 / 120);
  const buffersBefore = snapshotBuffers(rig);
  const supportsBefore = snapshotSupports(rig);
  const limbsBefore = JSON.stringify(rig.limbs);
  const clawsBefore = JSON.stringify(rig.claws);

  assert.equal(R.setChannelTarget(rig, "head-yaw", NaN), true);
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.deepEqual(snapshotBuffers(rig), buffersBefore);
  assert.deepEqual(snapshotSupports(rig), supportsBefore);
  assert.equal(JSON.stringify(rig.limbs), limbsBefore);
  assert.equal(JSON.stringify(rig.claws), clawsBefore);

  rig.cable[7] = Infinity;
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.deepEqual(snapshotBuffers(rig), buffersBefore);
  assert.deepEqual(snapshotSupports(rig), supportsBefore);
  assert.equal(rig.diagnostics.recoveries, 2);
});

test("failed solves restore every canonical and last-valid field except recovery count", () => {
  const rig = R.createRig(171);
  for(let i = 0; i < 20; i += 1) assert.equal(R.solveRig(rig, 1 / 120), true);
  const before = snapshotCanonical(rig);
  const recoveries = rig.diagnostics.recoveries;

  rig.lastDt = Infinity;
  assert.equal(R.solveRig(rig, 1 / 30), false);
  assert.deepEqual(snapshotCanonical(rig), before);
  assert.equal(rig.diagnostics.recoveries, recoveries + 1);
});

test("descriptor sabotage is rejected before any transition mutation", () => {
  const cases = [
    ["root coordinate", rig => {
      Object.defineProperty(rig.limbs["front-left"].root, "x", {writable:false});
    }],
    ["support load", rig => {
      assert.equal(R.requestGrip(rig, "front-right", "loaded", 0.72), true);
      Object.defineProperty(rig.supports["front-right"], "load", {writable:false});
    }],
    ["claw mode", rig => {
      Object.defineProperty(rig.claws["claw-front-left-1"], "mode", {writable:false});
    }],
    ["frozen diagnostics", rig => { Object.freeze(rig.diagnostics); }],
    ["frozen rig", rig => { Object.freeze(rig); }],
    ["late snapshot scalar", rig => {
      assert.equal(R.setChannelTarget(rig, "jaw-open", 0.9), true);
      Object.defineProperty(rig, "lastValidCableEnergy", {writable:false});
    }],
  ];
  for(const [label, sabotage] of cases){
    const rig = R.createRig(`descriptor-${label}`);
    sabotage(rig);
    const before = snapshotCanonical(rig);
    const recoveries = rig.diagnostics.recoveries;
    assert.doesNotThrow(() => assert.equal(R.solveRig(rig, 1 / 60), false), label);
    assert.deepEqual(snapshotCanonical(rig), before, label);
    assert.equal(rig.diagnostics.recoveries, recoveries, label);
  }
});

test("unrestorable non-writable poison remains total without further mutation", () => {
  const rig = R.createRig("non-writable-poison");
  rig.limbs["front-left"].root.x = NaN;
  Object.defineProperty(rig.limbs["front-left"].root, "x", {writable:false});
  const before = snapshotCanonical(rig);
  const recoveries = rig.diagnostics.recoveries;
  assert.doesNotThrow(() => assert.equal(R.solveRig(rig, 1 / 60), false));
  assert.deepEqual(snapshotCanonical(rig), before);
  assert.equal(rig.diagnostics.recoveries, recoveries);
});

test("construction prevents deep topology and accessor replacement", () => {
  const rig = R.createRig("sealed-topology");
  const support = rig.supports["front-left"];
  const limb = rig.limbs["front-left"];
  const claw = rig.claws["claw-front-left-1"];
  assert.equal(Object.isSealed(rig), true);
  assert.equal(Object.isSealed(rig.diagnostics), true);
  assert.equal(Object.isFrozen(rig.supports), true);
  assert.equal(Object.isFrozen(rig.limbs), true);
  assert.equal(Object.isFrozen(rig.claws), true);
  assert.equal(Object.isSealed(support), true);
  assert.equal(Object.isSealed(limb), true);
  assert.equal(Object.isSealed(claw), true);
  assert.equal(Object.isSealed(support.point), true);
  assert.equal(Object.isSealed(limb.joint), true);
  assert.equal(Object.isSealed(claw.point), true);
  assert.deepEqual(Reflect.ownKeys(support),
    ["limb", "mode", "cableT", "load", "point", "normal"]);
  assert.deepEqual(Reflect.ownKeys(claw),
    ["id", "limb", "index", "mode", "cableT", "point", "contactError"]);

  const symbol = Symbol("extra");
  assert.equal(Reflect.defineProperty(rig.supports, symbol, {value:support}), false);
  assert.equal(Reflect.defineProperty(rig.limbs, "hidden", {value:limb}), false);
  assert.equal(Reflect.set(claw.point, "z", 1), false);
  assert.equal(Reflect.defineProperty(support, "hidden", {value:true}), false);
  assert.equal(Reflect.defineProperty(support.point, "x", {get(){ return 76; }}), false);
  assert.equal(Reflect.set(rig, "supports", {...rig.supports}), false);
  assert.equal(Reflect.set(support, "point", {...support.point}), false);
  assert.equal(Reflect.set(limb, "joint", {...limb.joint}), false);
  assert.equal(Reflect.set(claw, "point", {...claw.point}), false);
  assert.deepEqual(R.validatePose(rig), []);
  assert.equal(R.solveRig(rig, 1 / 120), true);
});

test("mutable malformed state and poisoned snapshots recover without throwing", () => {
  const corruptions = [
    ["support point", rig => { rig.supports["front-left"].point.x = NaN; }],
    ["claw point", rig => { rig.claws["claw-front-left-1"].point.x = NaN; }],
    ["limb target", rig => { rig.limbs["front-left"].target.x = NaN; }],
    ["recovery count", rig => { rig.diagnostics.recoveries = NaN; }],
    ["last-valid dt", rig => { rig.lastValidDt = Infinity; }],
    ["last-valid limb snapshot", rig => { rig.lastValidLimbs[0] = NaN; }],
  ];
  for(const [label, corrupt] of corruptions){
    const rig = R.createRig(`malformed-${label}`);
    const before = snapshotCanonical(rig);
    corrupt(rig);
    if(label.startsWith("last-valid")) rig.targets[R.channelIndex("head-yaw")] = NaN;
    assert.doesNotThrow(() => assert.equal(R.solveRig(rig, 1 / 60), false), label);
    assert.deepEqual(snapshotCanonical(rig), before, label);
    assert.equal(rig.diagnostics.recoveries, 1, label);
  }
});

test("frozen maps prevent extra anatomy topology without changing the pose", () => {
  const rig = R.createRig(172);
  const beforeHash = R.poseHash(rig);
  assert.equal(Reflect.set(rig.claws, "extra", rig.claws["claw-front-left-1"]), false);
  assert.equal(Reflect.defineProperty(rig.supports, Symbol("extra"),
    {value:rig.supports["front-left"]}), false);
  assert.equal(Object.keys(rig.claws).length, 12);
  assert.equal(R.poseHash(rig), beforeHash);
  assert.deepEqual(R.validatePose(rig), []);
  assert.equal(R.solveRig(rig, 0), true);
});

test("public validation is total for every hostile top-level container", () => {
  const valid = R.createRig("validation-hostile");
  const fixtures = [null, {}, {values:new Float64Array(120)}];
  const topContainers = ["diagnostics", "supports", "limbs", "claws", "clawGroups",
    "clawList", "values", "targets", "velocities", "lastValidValues",
    "lastValidTargets", "lastValidVelocities", "cable", "cablePrevious",
    "cableRestLengths", "lastValidCable", "lastValidCablePrevious", "lastValidSupports",
    "lastValidLimbs", "lastValidClaws"];
  for(const field of topContainers) fixtures.push({...valid, [field]:null});
  for(let index = 0; index < fixtures.length; index += 1){
    assert.doesNotThrow(() => assert.ok(R.validatePose(fixtures[index]).length > 0),
      `fixture ${index}`);
  }
});

test("public validation covers every canonical live and last-valid family", () => {
  const corruptions = [
    ["limb root", rig => { rig.limbs["front-left"].root.x = NaN; }],
    ["limb target", rig => { rig.limbs["front-left"].target.x = NaN; }],
    ["finite limb target", rig => { rig.limbs["front-left"].target.x += 0.25; }],
    ["limb joint", rig => { rig.limbs["front-left"].joint.x = NaN; }],
    ["limb end", rig => { rig.limbs["front-left"].end.x = NaN; }],
    ["upper angle", rig => { rig.limbs["front-left"].upperAngle = NaN; }],
    ["finite upper angle", rig => { rig.limbs["front-left"].upperAngle += 0.01; }],
    ["lower angle", rig => { rig.limbs["front-left"].lowerAngle = Infinity; }],
    ["finite lower angle", rig => { rig.limbs["front-left"].lowerAngle += 0.01; }],
    ["limb contact", rig => { rig.limbs["front-left"].contactError = NaN; }],
    ["recoveries", rig => { rig.diagnostics.recoveries = -1; }],
    ["steps", rig => { rig.diagnostics.steps = 0.5; }],
    ["stretch metric", rig => { rig.diagnostics.maxCableStretch = Infinity; }],
    ["contact metric", rig => { rig.diagnostics.maxLoadedContactError = Infinity; }],
    ["energy metric", rig => { rig.diagnostics.cableEnergy = NaN; }],
    ["last-valid values", rig => { rig.lastValidValues[0] = NaN; }],
    ["last-valid targets", rig => { rig.lastValidTargets[0] = NaN; }],
    ["last-valid velocities", rig => { rig.lastValidVelocities[0] = NaN; }],
    ["last-valid cable", rig => { rig.lastValidCable[0] = NaN; }],
    ["last-valid cable previous", rig => { rig.lastValidCablePrevious[0] = NaN; }],
    ["last-valid supports", rig => { rig.lastValidSupports[0] = NaN; }],
    ["last-valid limbs", rig => { rig.lastValidLimbs[0] = NaN; }],
    ["last-valid claws", rig => { rig.lastValidClaws[0] = NaN; }],
    ["last-valid transfer", rig => { rig.lastValidTransferGoal = 2; }],
    ["last-valid dt", rig => { rig.lastValidDt = NaN; }],
    ["last-valid steps", rig => { rig.lastValidSteps = -1; }],
    ["last-valid stretch", rig => { rig.lastValidMaxCableStretch = NaN; }],
    ["last-valid contact", rig => { rig.lastValidMaxLoadedContactError = NaN; }],
    ["last-valid energy", rig => { rig.lastValidCableEnergy = NaN; }],
  ];
  for(const [label, corrupt] of corruptions){
    const rig = R.createRig(`validation-${label}`);
    corrupt(rig);
    assert.doesNotThrow(() => assert.ok(R.validatePose(rig).length > 0), label);
  }

  const detached = R.createRig("validation-detached-buffer");
  structuredClone(detached.lastValidValues.buffer,
    {transfer:[detached.lastValidValues.buffer]});
  assert.doesNotThrow(() => assert.ok(R.validatePose(detached).length > 0));
  assert.doesNotThrow(() => assert.equal(R.solveRig(detached, 1 / 60), false));
});

test("transfer goal names a currently loaded support or no support", () => {
  const released = R.createRig("released-transfer-goal");
  released.transferGoal = 1;
  assert.ok(R.validatePose(released).some(error => error.includes("transfer goal")));
  assert.equal(R.solveRig(released, 1 / 60), false);
  assert.equal(released.transferGoal, -1);
  assert.equal(released.diagnostics.recoveries, 1);

  const loaded = R.createRig("loaded-transfer-goal");
  loaded.transferGoal = 0;
  assert.deepEqual(R.validatePose(loaded), []);
  assert.equal(R.solveRig(loaded, 1 / 120), true);
});

test("private authored configuration prevents structural mutation and restores mutable state", () => {
  const prevented = [
    ["upper length", rig => Reflect.set(rig.limbs["front-left"], "upperLength", 1)],
    ["bend branch", rig => Reflect.set(rig.limbs["front-right"], "bend", 1)],
    ["rest geometry", rig => Reflect.set(rig.limbs["rear-left"].restEnd, "x", 1)],
    ["cable iterations", rig => Reflect.set(rig, "cableIterations", 7)],
    ["support topology", rig => Reflect.set(rig.supports["front-left"], "limb", "rear-right")],
    ["claw id", rig => Reflect.set(rig.claws["claw-front-left-1"], "id", "broken")],
    ["claw limb", rig => Reflect.set(rig.claws["claw-front-left-2"], "limb", "rear-right")],
    ["claw index", rig => Reflect.set(rig.claws["claw-front-left-3"], "index", 99)],
  ];
  for(const [label, attempt] of prevented){
    const rig = R.createRig(`config-prevented-${label}`);
    const before = snapshotCanonical(rig);
    assert.equal(attempt(rig), false, label);
    assert.deepEqual(snapshotCanonical(rig), before, label);
    assert.equal(R.solveRig(rig, 1 / 120), true, label);
    assert.equal(rig.diagnostics.recoveries, 0, label);
  }

  const recoverable = [
    ["cable rest geometry", rig => { rig.cableRestLengths[0] += 1; }],
    ["contact metric", rig => { rig.diagnostics.maxLoadedContactError = NaN; }],
    ["transfer goal", rig => { rig.transferGoal = 99; }],
  ];
  for(const [label, corrupt] of recoverable){
    const rig = R.createRig(`config-${label}`);
    const before = snapshotCanonical(rig);
    corrupt(rig);
    assert.doesNotThrow(() => {
      assert.equal(R.solveRig(rig, 1 / 60), false, label);
    }, label);
    assert.deepEqual(snapshotCanonical(rig), before, label);
    assert.equal(rig.diagnostics.recoveries, 1, label);
    assert.equal(R.solveRig(rig, 1 / 120), true, `${label} recovery remains usable`);
  }
});

test("invalid support or loaded-claw contact is rejected before solver mutation", () => {
  const rig = R.createRig(18);
  const beforeHash = R.poseHash(rig);
  const supportsBefore = snapshotSupports(rig);
  rig.supports["front-left"].point.x = NaN;
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.equal(R.poseHash(rig), beforeHash);
  assert.deepEqual(snapshotSupports(rig), supportsBefore);
  assert.equal(rig.diagnostics.recoveries, 1);

  const claw = rig.claws["claw-front-left-1"];
  claw.point.x += 2;
  assert.ok(R.validatePose(rig).some(error => error.includes("claw-front-left-1")));
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.equal(R.poseHash(rig), beforeHash);
  assert.equal(rig.diagnostics.recoveries, 2);

  rig.limbs["front-left"].contactError = NaN;
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.equal(R.poseHash(rig), beforeHash);
  assert.equal(rig.diagnostics.recoveries, 3);

  rig.supports["front-left"].point.x += 0.25;
  assert.ok(R.validatePose(rig).some(error => error.includes("cable contact point")));
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.equal(R.poseHash(rig), beforeHash);

  rig.claws["claw-front-left-1"].cableT += 0.01;
  assert.ok(R.validatePose(rig).some(error => error.includes("claw-front-left-1")));
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.equal(R.poseHash(rig), beforeHash);

  rig.limbs["front-left"].contactError = 0.5;
  rig.claws["claw-front-left-1"].contactError = 0.5;
  assert.ok(R.validatePose(rig).some(error => error.includes("stored contact error")));
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.equal(R.poseHash(rig), beforeHash);
  assert.equal(rig.diagnostics.recoveries, 6);

  rig.supports["front-left"].point.x = NaN;
  assert.ok(R.validatePose(rig).some(error => error.includes("non-finite support point")));
});

test("pose hashes are deterministic and sensitive to solved pose changes", () => {
  const a = R.createRig("same-seed");
  const b = R.createRig("same-seed");
  assert.equal(R.poseHash(a), R.poseHash(b));
  assert.match(R.poseHash(a), /^[0-9a-f]{8}$/);

  R.setChannelTarget(a, "jaw-open", 0.9);
  R.solveRig(a, 1 / 60);
  assert.notEqual(R.poseHash(a), R.poseHash(b));
  R.setChannelTarget(b, "jaw-open", 0.9);
  R.solveRig(b, 1 / 60);
  assert.equal(R.poseHash(a), R.poseHash(b));
});

test("pose hash covers every authoritative physical state family", async t => {
  const mutations = [
    ["support normal", rig => { rig.supports["front-left"].normal.x += 0.25; }],
    ["claw mode", rig => { rig.claws["claw-front-left-1"].mode = "released"; }],
    ["claw cable position", rig => { rig.claws["claw-front-left-1"].cableT += 0.01; }],
    ["claw point", rig => { rig.claws["claw-front-left-1"].point.x += 0.01; }],
    ["claw error", rig => { rig.claws["claw-front-left-1"].contactError += 0.01; }],
    ["transfer goal", rig => { rig.transferGoal = 0; }],
    ["limb target", rig => { rig.limbs["front-left"].target.x += 0.01; }],
    ["limb angle", rig => { rig.limbs["front-left"].upperAngle += 0.01; }],
    ["limb contact error", rig => { rig.limbs["front-left"].contactError += 0.01; }],
    ["cable rest geometry", rig => { rig.cableRestLengths[0] += 0.01; }],
    ["last dt", rig => { rig.lastDt += 0.001; }],
    ["last-valid limbs", rig => { rig.lastValidLimbs[0] += 0.01; }],
    ["physical diagnostics", rig => { rig.diagnostics.cableEnergy += 0.01; }],
  ];
  for(const [label, mutate] of mutations){
    await t.test(label, () => {
      const a = R.createRig("hash-family");
      const b = R.createRig("hash-family");
      assert.equal(R.poseHash(a), R.poseHash(b));
      mutate(a);
      assert.notEqual(R.poseHash(a), R.poseHash(b));
    });
  }
});

test("all authoritative container and nested references are immutable", () => {
  const rig = R.createRig("immutable-references");
  const stableFields = ["diagnostics", "values", "targets", "velocities",
    "lastValidValues", "lastValidTargets", "lastValidVelocities", "cable",
    "cablePrevious", "cableRestLengths", "lastValidCable", "lastValidCablePrevious",
    "lastValidSupports", "lastValidLimbs", "lastValidClaws", "supports", "limbs",
    "claws", "clawGroups", "clawList"];
  const before = R.poseHash(rig);
  assert.equal(Reflect.set(rig, "seed", rig.seed + 1), false);
  for(const field of stableFields){
    const value = rig[field];
    const replacement = value instanceof Float64Array ? value.slice() :
      Array.isArray(value) ? value.slice() : {...value};
    assert.equal(Reflect.set(rig, field, replacement), false, field);
  }
  const support = rig.supports["front-left"];
  const limb = rig.limbs["front-left"];
  const claw = rig.claws["claw-front-left-1"];
  for(const field of ["point", "normal"]){
    assert.equal(Reflect.set(support, field, {...support[field]}), false, `support ${field}`);
  }
  for(const field of ["restRoot", "restJoint", "restEnd", "root", "target", "joint", "end"]){
    assert.equal(Reflect.set(limb, field, {...limb[field]}), false, `limb ${field}`);
  }
  assert.equal(Reflect.set(claw, "point", {...claw.point}), false);
  assert.equal(Reflect.set(rig.supports, "front-left", {...support}), false);
  assert.equal(Reflect.set(rig.limbs, "front-left", {...limb}), false);
  assert.equal(Reflect.set(rig.claws, "claw-front-left-1", {...claw}), false);
  assert.equal(R.poseHash(rig), before);
  assert.deepEqual(R.validatePose(rig), []);
});

test("pose hash includes writable descriptor integrity", async t => {
  const sabotages = [
    ["rig scalar", rig => Object.defineProperty(rig, "lastDt", {writable:false})],
    ["diagnostics", rig => Object.defineProperty(rig.diagnostics, "steps", {writable:false})],
    ["support", rig => Object.defineProperty(rig.supports["front-left"], "load", {writable:false})],
    ["limb point", rig => Object.defineProperty(rig.limbs["front-left"].root, "x", {writable:false})],
    ["claw", rig => Object.defineProperty(rig.claws["claw-front-left-1"], "mode", {writable:false})],
  ];
  for(const [label, sabotage] of sabotages){
    await t.test(label, () => {
      const rig = R.createRig(`hash-descriptor-${label}`);
      const before = R.poseHash(rig);
      sabotage(rig);
      assert.notEqual(R.poseHash(rig), before);
    });
  }
});

test("two-link IK configuration is derived from every authored anchor chain", () => {
  const rig = R.createRig(23);
  for(const [limbName, ids] of Object.entries(ART_CHAINS)){
    const expected = expectedChain(ids);
    const limb = rig.limbs[limbName];
    assert.deepEqual(limb.restRoot, expected.root);
    assert.deepEqual(limb.restJoint, expected.joint);
    assert.deepEqual(limb.restEnd, expected.end);
    assert.ok(Math.abs(limb.upperLength - expected.upperLength) < 1e-12);
    assert.ok(Math.abs(limb.lowerLength - expected.lowerLength) < 1e-12);
    assert.equal(limb.bend, expected.bend);
    assertPointNear(limb.root, expected.root);
    assert.ok(Number.isFinite(limb.upperAngle));
    assert.ok(Number.isFinite(limb.lowerAngle));
    if(rig.supports[limbName].mode === "released"){
      assertPointNear(limb.joint, expected.joint);
      assertPointNear(limb.end, expected.end);
    }
  }
  assert.ok(rig.diagnostics.maxLoadedContactError < 1);
});

test("authored rest pose hangs from two upper cable contacts with one curled free hind limb", () => {
  const rig = R.createRig("suspended-three-quarter");
  const loaded = Object.values(rig.supports).filter(support => support.mode === "loaded");
  assert.deepEqual(loaded.map(support => support.limb), ["front-left", "rear-right"]);
  const pelvis = artAnchor("pelvis");
  const skull = artAnchor("skull");
  const ribcage = artAnchor("ribcage");
  const lineX = pelvis.x - skull.x;
  const lineY = pelvis.y - skull.y;
  const lineAngle = Math.atan2(lineY, lineX) * 180 / Math.PI;
  assert.ok(lineX >= 28 && lineY >= 64,
    `skull-to-pelvis axis ${lineX},${lineY} is still upright instead of diagonal`);
  assert.ok(lineAngle >= 58 && lineAngle <= 72,
    `torso line angle ${lineAngle.toFixed(2)}deg does not read as a hanging diagonal`);
  assert.ok(ribcage.x >= skull.x && pelvis.x > ribcage.x + 20,
    "head, shoulder, and pelvis need a progressive three-quarter line of action");
  for(const support of loaded){
    const cable = sampleCable(rig, support.cableT,
      rig.values[R.channelIndex("cable-contact-bias")]);
    assertPointNear(support.point, cable, 1e-9);
    assert.ok(support.point.y < pelvis.y - 12,
      `${support.limb} contact ${support.point.y} is not above the suspended body`);
    assert.ok(rig.limbs[support.limb].contactError < 1);
  }
  const freeHind = rig.limbs["rear-left"];
  assert.ok(freeHind.end.y >= 156 && freeHind.end.y <= 168,
    `free hind end ${freeHind.end.y} must curl visibly below the rump without touching floor`);
  assert.ok(freeHind.joint.x < freeHind.root.x - 24 &&
    freeHind.joint.y > freeHind.root.y + 14 &&
    freeHind.end.x > freeHind.joint.x + 20 &&
    freeHind.end.y > freeHind.joint.y + 14,
  "free hind limb must fold back into a recognisable hook instead of extending like a leg");
  assert.ok(Math.abs(normalizedBranchCross(freeHind.root, freeHind.joint,
    freeHind.end)) > 0.3, "free hind limb must keep a visibly curled silhouette");
  assert.ok(pelvis.y > Math.max(...loaded.map(support => support.point.y)) + 20,
    "body center must remain suspended below both cable contacts");
});

test("every solved chain retains its independently authored bend branch", () => {
  const rig = R.createRig("authored-branches");
  for(const [limbName, ids] of Object.entries(ART_CHAINS)){
    const expectedSign = -expectedChain(ids).bend;
    const limb = rig.limbs[limbName];
    for(const [label, point] of [["end", limb.end], ["target", limb.target]]){
      const cross = normalizedBranchCross(limb.root, limb.joint, point);
      assert.ok(Number.isFinite(cross), `${limbName} ${label} branch is degenerate`);
      assert.ok(cross * expectedSign > 1e-8,
        `${limbName} ${label} branch ${cross} does not match ${expectedSign}`);
    }
  }
});

test("a coherent reflected IK branch is rejected and atomically recovered", () => {
  const rig = R.createRig("reflected-branch");
  const before = snapshotCanonical(rig);
  const beforeHash = R.poseHash(rig);
  const limb = rig.limbs["front-right"];
  const reflected = reflectAcrossRootEnd(limb.joint, limb.root, limb.end);
  limb.joint.x = reflected.x;
  limb.joint.y = reflected.y;
  limb.upperAngle = Math.atan2(limb.joint.y - limb.root.y,
    limb.joint.x - limb.root.x);
  const lowerWorld = Math.atan2(limb.end.y - limb.joint.y,
    limb.end.x - limb.joint.x);
  limb.lowerAngle = limb.upperAngle - lowerWorld;

  assert.ok(R.validatePose(rig).some(error => error.includes("IK branch")));
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.deepEqual(snapshotCanonical(rig), before);
  assert.equal(R.poseHash(rig), beforeHash);
  assert.equal(rig.diagnostics.recoveries, 1);
});

test("released chains solve back to their independently authored rest anchors", () => {
  const rearSafe = R.createRig(231);
  assert.equal(R.requestGrip(rearSafe, "rear-right", "loaded", 0.72), true);
  for(let i = 0; i < 240; i += 1) assert.equal(R.solveRig(rearSafe, 1 / 120), true);
  assert.equal(R.requestGrip(rearSafe, "front-left", "release", 0.34), true);
  for(const name of ["front-left", "front-right", "rear-left"]){
    const expected = expectedChain(ART_CHAINS[name]);
    assertPointNear(rearSafe.limbs[name].joint, expected.joint, 1e-8);
    assertPointNear(rearSafe.limbs[name].end, expected.end, 1e-8);
  }

  const frontSafe = R.createRig(232);
  assert.equal(R.requestGrip(frontSafe, "front-left", "loaded", 0.34), true);
  for(let i = 0; i < 240; i += 1) assert.equal(R.solveRig(frontSafe, 1 / 120), true);
  assert.equal(R.requestGrip(frontSafe, "rear-right", "release", 0.72), true);
  const expectedRear = expectedChain(ART_CHAINS["rear-right"]);
  assertPointNear(frontSafe.limbs["rear-right"].joint, expectedRear.joint, 1e-8);
  assertPointNear(frontSafe.limbs["rear-right"].end, expectedRear.end, 1e-8);
});

test("Art-derived body zones reject segment traversal, endpoints, and contacts", () => {
  const crossing = R.createRig(241);
  crossing.limbs["front-left"].joint.x = 85;
  crossing.limbs["front-left"].joint.y = 105;
  assert.ok(R.validatePose(crossing).some(error => error.includes("segment enters body")));

  const endpoint = R.createRig(242);
  endpoint.limbs["front-left"].end.x = 151;
  endpoint.limbs["front-left"].end.y = 129;
  assert.ok(R.validatePose(endpoint).some(error => error.includes("endpoint enters body")));

  const contact = R.createRig(243);
  contact.supports["front-left"].point.x = 107;
  contact.supports["front-left"].point.y = 107;
  contact.claws["claw-front-left-1"].point.x = 107;
  contact.claws["claw-front-left-1"].point.y = 107;
  assert.ok(R.validatePose(contact).some(error => error.includes("contact enters body")));

  assert.deepEqual(R.validatePose(R.createRig(244)), []);
});

test("a root inside its own Art attachment zone may exit once but not re-enter", () => {
  const rig = R.createRig(245);
  const limb = rig.limbs["front-right"];
  limb.root.x = 129;
  limb.root.y = 70;
  limb.joint.x = limb.root.x + limb.upperLength;
  limb.joint.y = 70;
  limb.end.x = limb.joint.x + limb.lowerLength;
  limb.end.y = 70;
  assert.equal(R.validatePose(rig).some(error => error.includes("body overlap zone")), false);

  limb.end.x = 120;
  limb.end.y = 70;
  assert.ok(R.validatePose(rig).some(error => error.includes("endpoint enters body")));
});

test("validation rejects broken bone lengths and non-unit contact normals", () => {
  const rig = R.createRig(24);
  rig.limbs["front-left"].joint.x += 0.25;
  assert.ok(R.validatePose(rig).some(error => error.includes("bone length")));
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.deepEqual(R.validatePose(rig), []);

  rig.supports["rear-right"].normal.y = -0.5;
  assert.ok(R.validatePose(rig).some(error => error.includes("unit normal")));
  assert.equal(R.solveRig(rig, 1 / 60), false);
  assert.deepEqual(R.validatePose(rig), []);
  assert.equal(rig.diagnostics.recoveries, 2);
});

test("solveRig preserves buffer and anatomy identities during steady state", () => {
  const rig = R.createRig(29);
  const refs = {
    values:rig.values, targets:rig.targets, velocities:rig.velocities,
    lastValidValues:rig.lastValidValues, lastValidTargets:rig.lastValidTargets,
    lastValidVelocities:rig.lastValidVelocities, cable:rig.cable,
    cablePrevious:rig.cablePrevious, cableRestLengths:rig.cableRestLengths,
    lastValidCable:rig.lastValidCable,
    lastValidCablePrevious:rig.lastValidCablePrevious,
    lastValidSupports:rig.lastValidSupports, lastValidLimbs:rig.lastValidLimbs,
    lastValidClaws:rig.lastValidClaws, clawGroups:rig.clawGroups,
    clawList:rig.clawList,
    supports:rig.supports, frontLeft:rig.supports["front-left"],
    frontLeftPoint:rig.supports["front-left"].point,
    limbs:rig.limbs, frontLeftLimb:rig.limbs["front-left"], claws:rig.claws,
  };
  for(let i = 0; i < 10000; i += 1) assert.equal(R.solveRig(rig, 1 / 120), true);
  for(const [name, reference] of Object.entries(refs)){
    const current = name === "frontLeft" ? rig.supports["front-left"] :
      name === "frontLeftPoint" ? rig.supports["front-left"].point :
      name === "frontLeftLimb" ? rig.limbs["front-left"] : rig[name];
    assert.equal(current, reference, `${name} identity changed`);
  }
  assert.equal(rig.diagnostics.recoveries, 0);
});

test("solve helpers use frozen precomputed claw references without dynamic keys", () => {
  const rig = R.createRig(291);
  assert.equal(Object.isFrozen(rig.clawGroups), true);
  assert.equal(Object.isFrozen(rig.clawList), true);
  assert.equal(rig.clawGroups.length, 4);
  assert.equal(rig.clawList.length, 12);
  for(const group of rig.clawGroups) assert.equal(Object.isFrozen(group), true);
  const groups = rig.clawGroups;
  const list = rig.clawList;
  for(let i = 0; i < 100; i += 1) assert.equal(R.solveRig(rig, 1 / 120), true);
  assert.equal(rig.clawGroups, groups);
  assert.equal(rig.clawList, list);

  const hotStart = RIG_SOURCE.indexOf("function cablePoint");
  const hotEnd = RIG_SOURCE.indexOf("function expectedBuffer");
  const restoreStart = hotEnd;
  const restoreEnd = RIG_SOURCE.indexOf("function createRig");
  const gripStart = RIG_SOURCE.indexOf("function requestGrip");
  const solveStart = RIG_SOURCE.indexOf("function solveRig");
  const solveEnd = RIG_SOURCE.indexOf("function validatePose");
  assert.ok(hotStart >= 0 && hotEnd > hotStart);
  assert.ok(restoreEnd > restoreStart);
  assert.ok(gripStart >= 0 && solveStart > gripStart);
  assert.ok(solveStart >= 0 && solveEnd > solveStart);
  const solveSource = RIG_SOURCE.slice(solveStart, solveEnd);
  assert.match(solveSource, /function solveRig/);
  const helperSource = RIG_SOURCE.slice(hotStart, hotEnd);
  assert.match(helperSource, /function cablePoint/);
  assert.match(helperSource, /function solveLimb/);
  assert.match(helperSource, /function solveAllLimbs/);
  assert.match(helperSource, /function updateClaws/);
  const restoreSource = RIG_SOURCE.slice(restoreStart, restoreEnd);
  const gripSource = RIG_SOURCE.slice(gripStart, solveStart);
  assert.match(restoreSource, /function restoreLastValid/);
  assert.match(gripSource, /function requestGrip/);
  const steadySource = helperSource + restoreSource + gripSource + solveSource;
  assert.doesNotMatch(steadySource, /`claw-\$\{/);
  assert.doesNotMatch(steadySource, /\bnew\s+/);
  assert.doesNotMatch(steadySource, /=\s*\[\s*\]/);
  assert.doesNotMatch(steadySource, /\.map\s*\(/);
  assert.doesNotMatch(steadySource, /Object\.(?:keys|values|entries)\s*\(/);
});

test("thirty-minute deterministic cable swing stays constrained and in contact", () => {
  const rig = R.createRig(31);
  let maxStretch = 0;
  let maxContactError = 0;
  let maxEnergy = 0;
  const dt = 1 / 60;
  const frames = 30 * 60 * 60;

  for(let frame = 0; frame < frames; frame += 1){
    R.setChannelTarget(rig, "cable-wind", Math.sin(frame * 0.013) * 0.85);
    R.setChannelTarget(rig, "cable-pulse", Math.sin(frame * 0.007) * 0.7);
    assert.equal(R.solveRig(rig, dt), true, `recovery at frame ${frame}`);
    maxStretch = Math.max(maxStretch, rig.diagnostics.maxCableStretch);
    maxContactError = Math.max(maxContactError, rig.diagnostics.maxLoadedContactError);
    maxEnergy = Math.max(maxEnergy, rig.diagnostics.cableEnergy);
  }

  assert.ok(maxStretch < 0.03, `maximum stretch ${maxStretch}`);
  assert.ok(Number.isFinite(maxEnergy), `energy ${maxEnergy}`);
  assert.ok(maxContactError < 1, `maximum contact error ${maxContactError}`);
  assert.equal(rig.diagnostics.recoveries, 0);

  const repeat = R.createRig(31);
  for(let frame = 0; frame < frames; frame += 1){
    R.setChannelTarget(repeat, "cable-wind", Math.sin(frame * 0.013) * 0.85);
    R.setChannelTarget(repeat, "cable-pulse", Math.sin(frame * 0.007) * 0.7);
    R.solveRig(repeat, dt);
  }
  assert.equal(R.poseHash(repeat), R.poseHash(rig));
});
