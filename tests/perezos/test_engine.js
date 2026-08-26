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

  class IntersectionObserver {
    constructor(callback){
      this.callback = callback;
      this.observed = new Set();
      this.disconnected = false;
      intersections.push(this);
      observerRecords.push(this);
    }
    observe(target){ this.observed.add(target); }
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
    IntersectionObserver,
    ResizeObserver,
    matchMedia(){ return media; },
    now(){ return now; },
    sampleCost(kind, measured){ return costs[kind] ?? measured; },
    canvasFactory(width, height){
      if(options.decodeFailure) throw new Error("synthetic atlas decode failure");
      const offscreen = canvasFixture(width, height);
      return offscreen;
    },
    frames:{advance, pending(){ return callbacks.size; }},
    intersections,
    resizes,
    media,
    costs,
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

test("engine exports createPerezOS and the controller exact public API", () => {
  assert.equal(typeof E, "function", "ComandOSPerezOS.createPerezOS is undefined");
  const env = fakeEnvironment();
  const controller = E(env.canvas, {env});
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
    const controller = E(env.canvas, {env});
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
    const controller = E(env.canvas, {env});
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

  test("session changes reset living state but recreate a stable deterministic personality", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
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

  test("visible scheduler uses rAF as wakeup and keeps bounded visible time", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
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
    const controller = E(env.canvas, {env});
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
    const controller = E(env.canvas, {env});
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

  test("pointer proximity is sampled at no more than 10 Hz and drives habituation", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
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

  test("activation is cooldown-limited while repeated acknowledgements habituate", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
    controller.setContext(context());
    assert.equal(controller.notifyInteraction("activate", 128, 104), true);
    assert.equal(controller.notifyInteraction("click", 128, 104), false);
    const first = controller.getDiagnostics().habituation;
    env.frames.advance(800);
    assert.equal(controller.notifyInteraction("activate", 128, 104), true);
    assert.ok(controller.getDiagnostics().habituation > first);
    controller.destroy();
  });

  test("reduced motion and its media listener force event-driven Static quality", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
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
    const controller = E(env.canvas, {env});
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

  test("device and compact viewport ceilings prevent inappropriate upgrades", () => {
    const weakEnv = fakeEnvironment({hardwareConcurrency:2, deviceMemory:2});
    const weak = E(weakEnv.canvas, {env:weakEnv});
    assert.equal(weak.getDiagnostics().qualityCeiling, "economy");
    assert.equal(weak.getDiagnostics().quality, "economy");
    weak.setViewport(120, 90, 3);
    assert.equal(weak.getDiagnostics().qualityCeiling, "economy");
    weak.destroy();

    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
    controller.setViewport(160, 130, 2);
    assert.equal(controller.getDiagnostics().qualityCeiling, "economy");
    assert.equal(controller.getDiagnostics().quality, "economy");
    controller.destroy();
  });

  test("atlas decode failure enters a one-shot compact static fallback", () => {
    const env = fakeEnvironment({decodeFailure:true});
    const controller = E(env.canvas, {env});
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

  test("hot-loop buffers stay stable and diagnostics report measured counters", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
    controller.setContext(context());
    env.frames.advance(5000);
    const diagnostics = controller.getDiagnostics();
    assert.equal(diagnostics.hotLoopBufferReplacements, 0);
    assert.equal(diagnostics.timings.capacity, 240);
    assert.ok(diagnostics.timings.count > 0);
    assert.ok(diagnostics.decodedBytes > 0);
    assert.equal(diagnostics.listenerCount, 2);
    assert.equal(diagnostics.observerCount, 2);
    controller.destroy();
  });

  test("destroy is idempotent and leaves no callbacks, listeners, or observers", () => {
    const env = fakeEnvironment();
    const controller = E(env.canvas, {env});
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
