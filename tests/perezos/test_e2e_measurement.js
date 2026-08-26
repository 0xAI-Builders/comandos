"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");

const {temporalCoverage} = require("../e2e_perezos.js");

test("temporal coverage intersects trace timestamps with the measured browser window", () => {
  assert.deepEqual(temporalCoverage({timestamp:[99.5,130]}, 100, 130.5), {
    windowMs:30.5,
    coverageMs:30,
    startLagMs:0,
    endLagMs:0.5,
  });
  assert.deepEqual(temporalCoverage({timestamp:[101,132]}, 100, 130), {
    windowMs:30,
    coverageMs:29,
    startLagMs:1,
    endLagMs:0,
  });
});
