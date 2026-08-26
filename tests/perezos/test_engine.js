"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");
require("../../dash/perezos/rig.js");
require("../../dash/perezos/behaviors.js");
require("../../dash/perezos/motion.js");
require("../../dash/perezos/renderer.js");

const enginePath = path.resolve(__dirname, "../../dash/perezos/engine.js");
if(fs.existsSync(enginePath)) require(enginePath);

const NS = global.ComandOSPerezOS;
const E = NS.createPerezOS;

function eventTarget(initial = {}){
  const listeners = new Map();
  return Object.assign(initial, {
    addEventListener(type, listener){
      if(!listeners.has(type)) listeners.set(type, new Set());
      listeners.get(type).add(listener);
    },
    removeEventListener(type, listener){
      const set = listeners.get(type);
      if(set) set.delete(listener);
    },
    emit(type, event = {}){
      const set = listeners.get(type);
      if(set) for(const listener of Array.from(set)) listener(event);
    },
    listenerCount(type){ return listeners.get(type)?.size || 0; },
    totalListeners(){
      let count = 0;
      for(const set of listeners.values()) count += set.size;
      return count;
    },
  });
}

function recordingContext(){
  return {
    fillStyle:"", imageSmoothingEnabled:true, clears:0, fills:0, draws:0,
    clearRect(){ this.clears += 1; },
    fillRect(){ this.fills += 1; },
    drawImage(){ this.draws += 1; },
    setTransform(){},
  };
}

function canvasFixture(width = 256, height = 208){
  const context2d = recordingContext();
  return {
    width, height, clientWidth:width, clientHeight:height, context2d,
    getContext(kind){ return kind === "2d" ? context2d : null; },
  };
}

function fakeEnvironment(options = {}){
  let now = 0;
  let frameId = 1;
  const callbacks = new Map();
  const document = eventTarget({hidden:false});
  const media = eventTarget({matches:false});
  const canvas = canvasFixture(options.width || 256, options.height || 208);
  const intersections = [];
  const resizes = [];
  const observerRecords = [];
  const costs = {update:0, render:0};
  const decodeLatch = {failed:false, attempts:0};
  let decodeAttempts = 0;

  class IntersectionObserver {
    constructor(callback){
      this.callback = callback;
      this.observed = new Set();
      this.disconnected = false;
      intersections.push(this);
      observerRecords.push(this);
    }
    observe(target){
      this.observed.add(target);
      if(options.autoIntersect !== false) this.emit(true);
    }
    disconnect(){ this.disconnected = true; this.observed.clear(); }
    emit(isIntersecting){
      this.callback([{target:canvas, isIntersecting, intersectionRatio:isIntersecting ? 1 : 0}]);
    }
  }

  class ResizeObserver {
    constructor(callback){
      this.callback = callback;
      this.observed = new Set();
      this.disconnected = false;
      resizes.push(this);
      observerRecords.push(this);
    }
    observe(target){ this.observed.add(target); }
    disconnect(){ this.disconnected = true; this.observed.clear(); }
    emit(width, height){
      canvas.clientWidth = width;
      canvas.clientHeight = height;
      this.callback([{target:canvas, contentRect:{width, height}}]);
    }
  }

  function requestAnimationFrame(callback){
    const id = frameId++;
    callbacks.set(id, callback);
    return id;
  }

  function cancelAnimationFrame(id){ callbacks.delete(id); }

  function advance(milliseconds, frameMs = 1000 / 60){
    const end = now + milliseconds;
    while(now + 1e-9 < end){
      now = Math.min(end, now + frameMs);
      const ready = Array.from(callbacks.values());
      callbacks.clear();
      for(const callback of ready) callback(now);
    }
  }

  const env = {
    canvas,
    document,
    navigator:{
      hardwareConcurrency:options.hardwareConcurrency ?? 8,
      deviceMemory:options.deviceMemory ?? 8,
    },
    devicePixelRatio:options.dpr || 2,
    requestAnimationFrame,
    cancelAnimationFrame,
    IntersectionObserver:options.noIntersectionObserver ? undefined : IntersectionObserver,
    ResizeObserver,
    matchMedia(){ return media; },
    now(){ return now; },
    sampleCost(kind, measured){ return costs[kind] ?? measured; },
    canvasFactory(width, height){
      decodeAttempts += 1;
      if(options.decodeFailure) throw new Error("synthetic atlas decode failure");
      const offscreen = canvasFixture(width, height);
      return offscreen;
    },
    frames:{advance, pending(){ return callbacks.size; }},
    intersections,
    resizes,
    media,
    costs,
    decodeLatch,
    decodeAttempts(){ return decodeAttempts; },
    listenerCount(){ return document.totalListeners() + media.totalListeners(); },
    observerRecords,
  };
  return env;
}

function context(overrides = {}){
  return {
    sessionId:"session-name", status:"idle", role:"daily", costume:"",
    contextPressure:"low", theme:"noche", expanded:false, timestamp:0,
    colors:{brand:"#8B7CFF", panel:"#121722", line:"#222A3A"},
    ...overrides,
  };
}

function engineOptions(env){ return {env, decodeLatch:env.decodeLatch}; }

test("engine exports createPerezOS and the controller exact public API", () => {
  assert.equal(typeof E, "function", "ComandOSPerezOS.createPerezOS is undefined");
  const env = fakeEnvironment();
  const controller = E(env.canvas, engineOptions(env));
  assert.deepEqual(Object.keys(controller).sort(), [
    "destroy", "getDiagnostics", "notifyInteraction", "setContext",
    "setReducedMotion", "setViewport", "setVisible",
  ]);
  for(const name of Object.keys(controller)) assert.equal(typeof controller[name], "function");
  assert.equal(Object.isFrozen(controller), true);
  controller.destroy();
});

if(E){
  test("context is normalized to the exact deeply immutable primitive shape", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    const input = context({sessionId:42, status:"BOGUS", role:"  BUILD  ", costume:null,
      contextPressure:"impossible", expanded:1, timestamp:-8,
      colors:{brand:"bad", panel:"#abcdef", line:"#123456"}});
    assert.equal(controller.setContext(input), true);
    input.colors.panel = "#000000";
    const normalized = controller.getDiagnostics().context;
    assert.deepEqual(normalized, {
      sessionId:"42", status:"idle", role:"build", costume:"",
      contextPressure:"low", theme:"noche", expanded:true, timestamp:0,
      colors:{brand:"#8B7CFF", panel:"#ABCDEF", line:"#123456"},
    });
    assert.equal(Object.isFrozen(normalized), true);
    assert.equal(Object.isFrozen(normalized.colors), true);
    assert.throws(() => { normalized.status = "dead"; }, TypeError);
    controller.destroy();
  });

  test("same-session context changes retain controller, rig, renderer, and observers", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    const before = controller.getDiagnostics();
    assert.equal(controller.setContext(context({status:"working", timestamp:100})), true);
    const after = controller.getDiagnostics();
    assert.equal(after.controllerIdentity, before.controllerIdentity);
    assert.equal(after.rigIdentity, before.rigIdentity);
    assert.equal(after.rendererIdentity, before.rendererIdentity);
    assert.equal(after.sessionGeneration, before.sessionGeneration);
    assert.equal(env.intersections.length, 1);
    assert.equal(env.resizes.length, 1);
    assert.equal(env.document.listenerCount("visibilitychange"), 1);
    assert.equal(env.media.listenerCount("change"), 1);
    controller.destroy();
  });

  test("dashboard accent changes render without recreating same-session living state", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    env.frames.advance(120);
    const before = controller.getDiagnostics();
    assert.equal(controller.setContext(context({theme:"calido",
      colors:{brand:"#E0A458", panel:"#1F1811", line:"#36291A"}})), true);
    const changed = controller.getDiagnostics();
    assert.equal(changed.controllerIdentity, before.controllerIdentity);
    assert.equal(changed.rigIdentity, before.rigIdentity);
    assert.equal(changed.rendererIdentity, before.rendererIdentity);
    assert.equal(changed.sessionGeneration, before.sessionGeneration);
    env.frames.advance(120);
    assert.ok(controller.getDiagnostics().renders > before.renders);
    controller.destroy();
  });

  test("context diffs become bounded habituable Behavior perceptions", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    const changes = [
      {status:"working"},
      {status:"working", role:"build"},
      {status:"working", role:"build", contextPressure:"high"},
      {status:"working", role:"build", contextPressure:"high", expanded:true},
      {status:"working", role:"build", contextPressure:"high", expanded:true,
        theme:"dia"},
      {status:"working", role:"build", contextPressure:"high", expanded:true,
        theme:"dia", costume:"visor"},
    ];
    let current = context();
    for(const change of changes){
      current = {...current, ...change};
      assert.equal(controller.setContext(current), true);
    }
    const first = controller.getDiagnostics();
    assert.deepEqual(first.perceptions.byField, {
      status:1, role:1, contextPressure:1, expanded:1, theme:1, costume:1,
    });
    assert.equal(first.perceptions.total, 6);
    assert.ok(first.pendingBehaviorInteractions > 0);
    assert.ok(first.habituation > 0);
    const beforeRepeat = first.habituation;
    controller.setContext({...current, expanded:false});
    controller.setContext({...current, expanded:true});
    assert.ok(controller.getDiagnostics().habituation > beforeRepeat,
      "repeated same-field perceptions must habituate through Behaviors.notify");
    controller.destroy();
  });

  test("session changes reset living state but recreate a stable deterministic personality", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context({sessionId:"alpha"}));
    const alpha = controller.getDiagnostics();
    controller.setContext(context({sessionId:"beta"}));
    const beta = controller.getDiagnostics();
    controller.setContext(context({sessionId:"alpha"}));
    const alphaAgain = controller.getDiagnostics();
    assert.notEqual(beta.rigIdentity, alpha.rigIdentity);
    assert.equal(alphaAgain.personalitySignature, alpha.personalitySignature);
    assert.equal(alphaAgain.rendererIdentity, alpha.rendererIdentity);
    assert.equal(alphaAgain.sessionGeneration, alpha.sessionGeneration + 2);
    controller.destroy();
  });

  test("a new session restarts director and Motion on session-local visible time", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context({sessionId:"old-session", status:"working"}));
    env.frames.advance(5000);
    const old = controller.getDiagnostics();
    assert.ok(old.visibleTimeMs > 1000);
    assert.ok(old.sessionVisibleTimeMs > 1000);
    controller.setContext(context({sessionId:"fresh-session", status:"idle"}));
    const fresh = controller.getDiagnostics();
    assert.equal(fresh.sessionVisibleTimeMs, 0);
    assert.equal(fresh.behaviorVisibleTimeMs, 0);
    assert.equal(fresh.motionStartedAtMs, 0);
    assert.ok(fresh.visibleTimeMs >= old.visibleTimeMs,
      "controller-visible time remains a cumulative diagnostic");
    env.frames.advance(250);
    const advanced = controller.getDiagnostics();
    assert.ok(advanced.sessionVisibleTimeMs > 0 && advanced.sessionVisibleTimeMs <= 250);
    controller.destroy();
  });

  test("visible scheduler uses rAF as wakeup and keeps bounded visible time", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    env.frames.advance(1000);
    const diagnostics = controller.getDiagnostics();
    assert.ok(diagnostics.wakeups >= 50);
    assert.ok(diagnostics.updates >= 8 && diagnostics.updates <= 35, diagnostics.updates);
    assert.ok(diagnostics.renders > 0);
    assert.ok(diagnostics.visibleTimeMs <= 1000);
    assert.ok(diagnostics.maxStepMs <= 100);
    controller.destroy();
  });

  test("hidden controller performs zero work and does not replay backlog", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    env.frames.advance(1000);
    const before = controller.getDiagnostics();
    env.document.hidden = true;
    env.document.emit("visibilitychange");
    env.frames.advance(600000);
    const hidden = controller.getDiagnostics();
    assert.equal(hidden.updates, before.updates);
    assert.equal(hidden.renders, before.renders);
    assert.equal(env.frames.pending(), 0);
    env.document.hidden = false;
    env.document.emit("visibilitychange");
    env.frames.advance(100);
    const resumed = controller.getDiagnostics();
    assert.ok(resumed.updates - hidden.updates <= 4);
    assert.ok(resumed.maxStepMs <= 100);
    controller.destroy();
  });

  test("offscreen and preference stop cancel immediately and resume independently", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    env.frames.advance(500);
    env.intersections[0].emit(false);
    const offscreen = controller.getDiagnostics();
    env.frames.advance(10000);
    assert.equal(controller.getDiagnostics().updates, offscreen.updates);
    controller.setVisible(false);
    env.intersections[0].emit(true);
    env.frames.advance(1000);
    assert.equal(controller.getDiagnostics().updates, offscreen.updates);
    controller.setVisible(true);
    env.frames.advance(250);
    assert.ok(controller.getDiagnostics().updates > offscreen.updates);
    controller.destroy();
  });

  test("IntersectionObserver gates all work until its first sample", () => {
    const env = fakeEnvironment({autoIntersect:false});
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context({status:"working"}));
    assert.equal(env.frames.pending(), 0);
    env.frames.advance(1000);
    const beforeSample = controller.getDiagnostics();
    assert.equal(beforeSample.wakeups, 0);
    assert.equal(beforeSample.updates, 0);
    assert.equal(beforeSample.renders, 0);
    env.intersections[0].emit(true);
    env.frames.advance(250);
    assert.ok(controller.getDiagnostics().updates > 0);
    controller.destroy();

    const noObserverEnv = fakeEnvironment({noIntersectionObserver:true});
    const noObserver = E(noObserverEnv.canvas, engineOptions(noObserverEnv));
    noObserverEnv.frames.advance(250);
    assert.ok(noObserver.getDiagnostics().updates > 0,
      "without IntersectionObserver the visible controller must still run");
    noObserver.destroy();
  });

  test("pointer proximity is sampled at no more than 10 Hz and drives habituation", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    assert.equal(controller.notifyInteraction("pointer", 40, 40), true);
    for(let index = 0; index < 20; index += 1){
      assert.equal(controller.notifyInteraction("pointer", 42, 41), false);
    }
    env.frames.advance(100);
    assert.equal(controller.notifyInteraction("pointer", 42, 41), true);
    const diagnostics = controller.getDiagnostics();
    assert.equal(diagnostics.interactions.pointerAccepted, 2);
    assert.equal(diagnostics.interactions.pointerDropped, 20);
    assert.ok(diagnostics.habituation > 0);
    controller.destroy();
  });

  test("continuous sampled pointer activity sustains the Full action cadence", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context({status:"working"}));
    env.frames.advance(500);
    const before = controller.getDiagnostics().performanceTrace.sequenceEnd;
    for(let sample = 0; sample < 50; sample += 1){
      assert.equal(controller.notifyInteraction("pointer", sample % 2 ? 44 : 180, 72), true);
      env.frames.advance(7 * 16.6, 16.6);
    }
    const diagnostics = controller.getDiagnostics();
    const produced = diagnostics.performanceTrace.sequenceEnd - before;
    assert.ok(produced >= 170 && produced <= 180,
      `5.81 seconds of jittered active Full input must produce about 174 frames, got ${produced}`);
    assert.equal(diagnostics.quality, "full");
    const trace = controller.getDiagnostics({includePerformanceTrace:true}).performanceTrace;
    const from = before - trace.sequenceStart;
    assert.ok(trace.samples.active.slice(from).every(Boolean),
      "every frame in the continuous pointer window must be action cadence");
    controller.destroy();
  });

  test("activation is cooldown-limited while repeated acknowledgements habituate", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    assert.equal(controller.notifyInteraction("activate", 128, 104), true);
    assert.equal(controller.notifyInteraction("click", 128, 104), false);
    const first = controller.getDiagnostics().habituation;
    env.frames.advance(800);
    assert.equal(controller.notifyInteraction("activate", 128, 104), true);
    assert.ok(controller.getDiagnostics().habituation > first);
    controller.destroy();
  });

  test("accepted interaction requests a safe Motion interruption before it can expire", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    env.frames.advance(300);
    const before = controller.getDiagnostics();
    assert.ok(before.motionCurrentFamily, "fixture needs an active idle performance");
    assert.equal(controller.notifyInteraction("activate", 128, 104), true);
    const requested = controller.getDiagnostics();
    assert.ok(requested.motionPendingFamily || requested.motionCurrentState === "interaction",
      "Behavior interaction must be handed to Motion immediately");
    assert.equal(requested.interactionInterruptRequests, 1);
    env.frames.advance(5000);
    const reached = controller.getDiagnostics();
    assert.ok(reached.interactionAcknowledgementsStarted >= 1,
      "safe-boundary interruption must reach the acknowledgement before expiry");
    assert.equal(reached.solverRecoveries, 0);
    controller.destroy();
  });

  test("reduced motion and its media listener force event-driven Static quality", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    controller.setReducedMotion(true);
    env.frames.advance(1000);
    const manual = controller.getDiagnostics();
    assert.equal(manual.quality, "static");
    const updates = manual.updates;
    env.frames.advance(10000);
    assert.equal(controller.getDiagnostics().updates, updates);
    controller.setReducedMotion(false);
    env.media.matches = true;
    env.media.emit("change", {matches:true});
    assert.equal(controller.getDiagnostics().quality, "static");
    controller.destroy();
  });

  test("quality governor uses a 240-sample ring and 120/600-frame hysteresis", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context({status:"working"}));
    env.costs.update = 0.8;
    env.costs.render = 1.5;
    env.frames.advance(3000);
    assert.equal(controller.getDiagnostics().quality, "full", "must not downgrade early");
    for(let frame = 0; frame < 180 && controller.getDiagnostics().quality === "full";
        frame += 1){
      env.frames.advance(1000 / 60);
    }
    const degraded = controller.getDiagnostics();
    assert.equal(degraded.quality, "balanced");
    assert.equal(degraded.timings.capacity, 240);
    assert.ok(degraded.qualityTransitions >= 1);
    assert.ok(degraded.governorTransitions >= 1);
    env.costs.update = 0.1;
    env.costs.render = 0.1;
    env.frames.advance(20000);
    assert.equal(controller.getDiagnostics().quality, "balanced", "must not upgrade early");
    env.frames.advance(40000);
    assert.equal(controller.getDiagnostics().quality, "full");
    controller.destroy();
  });

  test("governor downgrade and upgrade preserve pose, contacts, phase, and RNG state", () => {
    const continuity = diagnostics => {
      assert.equal(typeof diagnostics.poseHash, "string");
      assert.equal(typeof diagnostics.contactSignature, "string");
      assert.equal(typeof diagnostics.randomState, "number");
      return {
        poseHash:diagnostics.poseHash,
        contactSignature:diagnostics.contactSignature,
        motionCurrentFamily:diagnostics.motionCurrentFamily,
        motionCurrentState:diagnostics.motionCurrentState,
        motionPhase:diagnostics.motionPhase,
        motionPhaseIndex:diagnostics.motionPhaseIndex,
        randomState:diagnostics.randomState,
      };
    };
    const advancePair = (left, right, milliseconds = 1000 / 60) => {
      left.frames.advance(milliseconds);
      right.frames.advance(milliseconds);
    };

    const high = fakeEnvironment();
    const baseline = fakeEnvironment();
    const highController = E(high.canvas, engineOptions(high));
    const baselineController = E(baseline.canvas, engineOptions(baseline));
    highController.setContext(context({status:"working"}));
    baselineController.setContext(context({status:"working"}));
    high.costs.update = 0.8; high.costs.render = 1.5;
    baseline.costs.update = 0.1; baseline.costs.render = 0.1;
    for(let frame = 0; frame < 1800 && highController.getDiagnostics().quality === "full";
        frame += 1){
      advancePair(high, baseline);
    }
    assert.equal(highController.getDiagnostics().quality, "balanced");
    assert.equal(baselineController.getDiagnostics().quality, "full");
    assert.deepEqual(continuity(highController.getDiagnostics()),
      continuity(baselineController.getDiagnostics()));
    highController.destroy();
    baselineController.destroy();

    const upgrade = fakeEnvironment();
    const balanced = fakeEnvironment();
    const upgradeController = E(upgrade.canvas, engineOptions(upgrade));
    const balancedController = E(balanced.canvas, engineOptions(balanced));
    upgradeController.setContext(context({status:"working"}));
    balancedController.setContext(context({status:"working"}));
    upgrade.costs.update = 0.8; upgrade.costs.render = 1.5;
    balanced.costs.update = 0.8; balanced.costs.render = 1.5;
    for(let frame = 0; frame < 1800 && upgradeController.getDiagnostics().quality === "full";
        frame += 1){
      advancePair(upgrade, balanced);
    }
    assert.equal(upgradeController.getDiagnostics().quality, "balanced");
    assert.equal(balancedController.getDiagnostics().quality, "balanced");
    upgrade.costs.update = 0.1; upgrade.costs.render = 0.1;
    balanced.costs.update = 0.4; balanced.costs.render = 0.4;
    for(let frame = 0; frame < 5000 && upgradeController.getDiagnostics().quality !== "full";
        frame += 1){
      advancePair(upgrade, balanced);
    }
    assert.equal(upgradeController.getDiagnostics().quality, "full");
    assert.equal(balancedController.getDiagnostics().quality, "balanced");
    assert.deepEqual(continuity(upgradeController.getDiagnostics()),
      continuity(balancedController.getDiagnostics()));
    upgradeController.destroy();
    balancedController.destroy();
  });

  test("device and compact viewport ceilings prevent inappropriate upgrades", () => {
    const weakEnv = fakeEnvironment({hardwareConcurrency:2, deviceMemory:2});
    const weak = E(weakEnv.canvas, engineOptions(weakEnv));
    assert.equal(weak.getDiagnostics().qualityCeiling, "economy");
    assert.equal(weak.getDiagnostics().quality, "economy");
    weak.setViewport(120, 90, 3);
    assert.equal(weak.getDiagnostics().qualityCeiling, "economy");
    weak.destroy();

    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setViewport(160, 130, 2);
    assert.equal(controller.getDiagnostics().qualityCeiling, "economy");
    assert.equal(controller.getDiagnostics().quality, "economy");
    controller.destroy();
  });

  test("viewport state commits only after Renderer acceptance and fallback enforces safe bounds", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    const before = controller.getDiagnostics();
    const oldWidth = env.canvas.width;
    let storedHeight = env.canvas.height;
    Object.defineProperty(env.canvas, "height", {
      configurable:true,
      get(){ return storedHeight; },
      set(value){
        if(value !== storedHeight) throw new Error("hostile height");
        storedHeight = value;
      },
    });
    assert.equal(controller.setViewport(120, 90, 2), false);
    const rejected = controller.getDiagnostics();
    assert.deepEqual(rejected.viewport, before.viewport);
    assert.equal(rejected.qualityCeiling, before.qualityCeiling);
    assert.equal(env.canvas.width, oldWidth, "Renderer rollback must restore backing width");
    controller.destroy();

    const fallbackEnv = fakeEnvironment({decodeFailure:true});
    const fallback = E(fallbackEnv.canvas, engineOptions(fallbackEnv));
    const fallbackBefore = fallback.getDiagnostics();
    const backing = [fallbackEnv.canvas.width, fallbackEnv.canvas.height];
    assert.equal(fallback.setViewport(9000, 9000, 4), false);
    assert.deepEqual(fallback.getDiagnostics().viewport, fallbackBefore.viewport);
    assert.deepEqual([fallbackEnv.canvas.width, fallbackEnv.canvas.height], backing);
    fallback.destroy();
  });

  test("atlas decode failure enters a one-shot compact static fallback", () => {
    const env = fakeEnvironment({decodeFailure:true});
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    env.frames.advance(100);
    const diagnostics = controller.getDiagnostics();
    assert.equal(diagnostics.decodeFailures, 1);
    assert.equal(diagnostics.fallback, true);
    assert.equal(diagnostics.quality, "static");
    assert.ok(env.canvas.context2d.fills > 0);
    controller.setContext(context({theme:"dia"}));
    env.frames.advance(100);
    assert.equal(controller.getDiagnostics().decodeFailures, 1, "must never retry decode");
    controller.destroy();
  });

  test("decode failure latch is shared across controllers without retrying", () => {
    const env = fakeEnvironment({decodeFailure:true});
    const first = E(env.canvas, engineOptions(env));
    assert.equal(env.decodeAttempts(), 1);
    assert.equal(first.getDiagnostics().decodeAttempts, 1);
    const secondCanvas = canvasFixture();
    const second = E(secondCanvas, engineOptions(env));
    assert.equal(env.decodeAttempts(), 1, "second controller must honor the page latch");
    assert.equal(second.getDiagnostics().fallback, true);
    assert.equal(second.getDiagnostics().decodeAttempts, 0);
    assert.equal(second.getDiagnostics().sharedDecodeFailure, true);
    first.destroy();
    second.destroy();
  });

  test("decode latch injection is isolated from host environment globals", () => {
    const env = fakeEnvironment();
    env.decodeLatch = Object.freeze({failed:true});
    const privateLatch = {failed:false};
    const controller = E(env.canvas, {env, decodeLatch:privateLatch});
    const diagnostics = controller.getDiagnostics();
    assert.equal(diagnostics.fallback, false,
      "an unrelated host global must not poison PerezOS decode");
    assert.equal(diagnostics.decodeAttempts, 1);
    assert.equal(privateLatch.failed, false);
    controller.destroy();
  });

  test("hot-loop buffers stay stable and diagnostics report measured counters", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    env.frames.advance(40000);
    const diagnostics = controller.getDiagnostics();
    assert.equal(diagnostics.stableBufferReplacements, 0);
    assert.equal(diagnostics.timings.capacity, 240);
    assert.ok(diagnostics.timings.count > 0);
    assert.ok(diagnostics.timings.update.count > 0);
    assert.ok(diagnostics.timings.render.count > 0);
    assert.ok(Number.isFinite(diagnostics.timings.update.average));
    assert.ok(Number.isFinite(diagnostics.timings.update.p95));
    assert.ok(Number.isFinite(diagnostics.timings.render.average));
    assert.ok(Number.isFinite(diagnostics.timings.render.p95));
    assert.equal(diagnostics.performanceTrace.capacity, 2048);
    assert.ok(diagnostics.performanceTrace.count > diagnostics.timings.capacity,
      "the validation trace must retain the complete 30-second window, not governor ring 240");
    assert.equal(diagnostics.performanceTrace.totalSamples,
      diagnostics.performanceTrace.count);
    assert.equal(diagnostics.performanceTrace.samples, undefined,
      "ordinary diagnostics must not materialize trace arrays");
    const traced = controller.getDiagnostics({includePerformanceTrace:true}).performanceTrace;
    assert.equal(traced.samples.combined.length, traced.count);
    assert.equal(traced.samples.update.length, traced.count);
    assert.equal(traced.samples.render.length, traced.count);
    assert.equal(traced.samples.timestamp.length, traced.count);
    assert.equal(traced.samples.active.length, traced.count);
    assert.equal(traced.samples.quality.length, traced.count);
    assert.ok(traced.samples.combined.every(Number.isFinite));
    assert.ok(traced.samples.update.every(Number.isFinite));
    assert.ok(traced.samples.render.every(Number.isFinite));
    assert.ok(traced.samples.timestamp.every(Number.isFinite));
    assert.ok(traced.samples.timestamp.every((value, index, values) =>
      index === 0 || value >= values[index - 1]));
    assert.ok(traced.samples.active.every(value => typeof value === "boolean"));
    assert.deepEqual([...new Set(traced.samples.quality)], ["full"]);
    assert.ok(diagnostics.engineBufferBytes >= 2048 * (8 + 8 + 8 + 8 + 1 + 1));
    assert.ok(diagnostics.decodedBytes > diagnostics.engineBufferBytes,
      "decoded budget includes both renderer atlas/cache and engine buffers");
    assert.equal(diagnostics.listenerCount, 2);
    assert.equal(diagnostics.observerCount, 2);
    controller.destroy();
  });

  test("steady timing and identity instrumentation is preallocated outside the frame loop", () => {
    const source = fs.readFileSync(enginePath, "utf8");
    const traceCreateStart = source.indexOf("function createPerformanceTrace");
    const tracePushStart = source.indexOf("function pushPerformanceTrace");
    const traceDiagnosticsStart = source.indexOf("function performanceTraceDiagnostics");
    assert.ok(traceCreateStart >= 0 && tracePushStart > traceCreateStart &&
      traceDiagnosticsStart > tracePushStart);
    const traceCreate = source.slice(traceCreateStart, tracePushStart);
    const tracePush = source.slice(tracePushStart, traceDiagnosticsStart);
    assert.equal((traceCreate.match(/new (?:Float64|Uint8)Array\(/g) || []).length, 6,
      "the complete-window trace owns six preallocated time/component/state buffers");
    assert.doesNotMatch(tracePush, /\bnew\s+|Array\.from|\.map\s*\(|\.slice\s*\(/,
      "recording one produced frame does not materialize new storage");
    const start = source.indexOf("function auditBufferIdentities");
    const end = source.indexOf("function updateCeiling");
    assert.ok(start >= 0 && end > start);
    const hotPath = source.slice(start, end);
    assert.doesNotMatch(hotPath, /\bnew\s+(?:Array|Float\d+Array|Int\d+Array|Uint\d+Array)/);
    assert.doesNotMatch(hotPath, /Array\.from|\.map\s*\(|\.slice\s*\(/);
  });

  test("destroy is idempotent and leaves no callbacks, listeners, or observers", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, engineOptions(env));
    controller.setContext(context());
    env.frames.advance(200);
    const before = controller.getDiagnostics();
    assert.equal(controller.destroy(), true);
    assert.equal(controller.destroy(), false);
    assert.equal(env.frames.pending(), 0);
    assert.equal(env.listenerCount(), 0);
    assert.ok(env.observerRecords.every(observer => observer.disconnected));
    env.frames.advance(1000);
    const destroyed = controller.getDiagnostics();
    assert.equal(destroyed.updates, before.updates);
    assert.equal(destroyed.renders, before.renders);
    assert.equal(destroyed.destroyed, true);
    assert.equal(destroyed.listenerCount, 0);
    assert.equal(destroyed.observerCount, 0);
  });
}
