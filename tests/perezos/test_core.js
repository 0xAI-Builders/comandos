"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
global.window = global;
require("../../dash/perezos/core.js");
const C = global.ComandOSPerezOS.Core;

test("seeded random streams are reproducible and forkable", () => {
  const a = C.createRng(C.hashSeed("session-a"));
  const b = C.createRng(C.hashSeed("session-a"));
  assert.deepEqual(Array.from({length: 64}, () => a.next()),
                   Array.from({length: 64}, () => b.next()));
  assert.notDeepEqual([a.fork("eyes").next(), a.fork("fur").next()],
                      [b.fork("cable").next(), b.fork("pose").next()]);
});

test("forked child streams persist and advance across repeated lookup", () => {
  const rng = C.createRng(C.hashSeed("session-a"));
  const eyes = rng.fork("eyes");
  const first = eyes.next();
  const second = rng.fork("eyes").next();
  const third = rng.fork("eyes").next();
  assert.equal(rng.fork("eyes"), eyes);
  assert.notEqual(first, second);
  assert.notEqual(second, third);
});

test("bounded spring converges without non-finite values", () => {
  const spring = C.createSpring(0, 90, 18);
  for (let i = 0; i < 600; i += 1) C.stepSpring(spring, 1, 1 / 120);
  assert.ok(Number.isFinite(spring.value));
  assert.ok(Number.isFinite(spring.velocity));
  assert.ok(Math.abs(spring.value - 1) < 0.001);
});

test("ring stats are fixed-size and report average and p95", () => {
  const stats = C.createRingStats(4);
  [1, 2, 30, 4, 5].forEach(value => stats.push(value));
  assert.equal(stats.count, 4);
  assert.equal(stats.average(), 10.25);
  assert.equal(stats.percentile(0.95), 30);
});

test("diagnostics use an injected clock and freeze timing snapshots", () => {
  const readings = [100, 103, 200, 205];
  const diagnostics = C.createDiagnostics(() => readings.shift());
  const updateStart = diagnostics.begin("update");
  assert.equal(diagnostics.end("update", updateStart), 3);
  const renderStart = diagnostics.begin("render");
  assert.equal(diagnostics.end("render", renderStart), 5);

  assert.deepEqual(diagnostics.counters, {update: 1, render: 1});
  const snapshot = diagnostics.snapshot();
  assert.equal(snapshot.update.average, 3);
  assert.equal(snapshot.update.p95, 3);
  assert.equal(snapshot.render.average, 5);
  assert.equal(snapshot.render.p95, 5);
  assert.equal(snapshot.updateAverage, 3);
  assert.equal(snapshot.updateP95, 3);
  assert.equal(snapshot.renderAverage, 5);
  assert.equal(snapshot.renderP95, 5);
  assert.equal(Object.isFrozen(snapshot), true);
  assert.equal(Object.isFrozen(snapshot.update), true);
  assert.equal(Object.isFrozen(snapshot.render), true);
});
