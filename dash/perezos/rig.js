(function(root){
  "use strict";

  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  if(!NS.Core) throw new Error("ComandOSPerezOS.Core must load before Rig");
  if(!NS.Art) throw new Error("ComandOSPerezOS.Art must load before Rig");

  const Core = NS.Core;
  const Art = NS.Art;
  const PI = Math.PI;
  const MAX_DT = 1 / 30;
  const CABLE_NODES = 9;
  const CABLE_SEGMENTS = CABLE_NODES - 1;
  const CABLE_ITERATIONS = 8;
  const MAX_CABLE_STRETCH = 0.03;
  const MAX_CONTACT_ERROR = 1;
  const SAFE_SINGLE_SUPPORT = 0.95;
  const SUPPORT_STRIDE = 7;
  const LIMB_STRIDE = 9;
  const CLAW_STRIDE = 5;
  const POINT_RESTORE_FIELDS = Object.freeze(["x", "y"]);
  const RIG_DYNAMIC_FIELDS = Object.freeze([
    "transferGoal", "lastValidTransferGoal", "lastDt", "lastValidDt",
    "lastValidSteps", "lastValidMaxCableStretch", "lastValidMaxLoadedContactError",
    "lastValidCableEnergy",
  ]);
  const DIAGNOSTIC_DYNAMIC_FIELDS = Object.freeze([
    "recoveries", "steps", "maxCableStretch", "maxLoadedContactError", "cableEnergy",
  ]);
  const SUPPORT_DYNAMIC_FIELDS = Object.freeze(["mode", "cableT", "load"]);
  const LIMB_DYNAMIC_FIELDS = Object.freeze(["upperAngle", "lowerAngle", "contactError"]);
  const CLAW_DYNAMIC_FIELDS = Object.freeze(["mode", "cableT", "contactError"]);
  const RIG_STABLE_FIELDS = Object.freeze([
    "seed", "values", "targets", "velocities", "lastValidValues", "lastValidTargets",
    "lastValidVelocities", "cable", "cablePrevious", "cableRestLengths",
    "lastValidCable", "lastValidCablePrevious", "lastValidSupports", "lastValidLimbs",
    "lastValidClaws", "supports", "limbs", "claws", "clawGroups", "clawList",
    "cableIterations", "diagnostics",
  ]);
  const PUBLIC_BUFFER_FIELDS = Object.freeze([
    "values", "targets", "velocities", "lastValidValues", "lastValidTargets",
    "lastValidVelocities", "cable", "cablePrevious", "cableRestLengths",
    "lastValidCable", "lastValidCablePrevious", "lastValidSupports", "lastValidLimbs",
    "lastValidClaws",
  ]);
  const AXIAL_ZONE_IDS = Object.freeze([
    "pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid", "neck-upper", "skull",
  ]);
  const RIG_INTERNALS = new WeakMap();

  const LIMB_NAMES = Object.freeze([
    "front-left", "front-right", "rear-left", "rear-right",
  ]);
  const LIMB_MANIFEST = Object.freeze([
    Object.freeze({name:"front-left", upper:"arm-fl-upper", lower:"arm-fl-fore",
      palm:"palm-fl"}),
    Object.freeze({name:"front-right", upper:"arm-fr-upper", lower:"arm-fr-fore",
      palm:"palm-fr"}),
    Object.freeze({name:"rear-left", upper:"leg-rl-upper", lower:"leg-rl-lower",
      palm:"palm-rl"}),
    Object.freeze({name:"rear-right", upper:"leg-rr-upper", lower:"leg-rr-lower",
      palm:"palm-rr"}),
  ]);

  const GROUP_SPECS = {
    axial:[
      ["spine-pelvis-x", -12, 12, 0], ["spine-pelvis-y", -12, 12, 0],
      ["spine-pelvis-angle", -0.45, 0.45, 0], ["spine-lower-angle", -0.55, 0.55, 0],
      ["spine-mid-angle", -0.65, 0.65, 0], ["spine-upper-angle", -0.55, 0.55, 0],
      ["neck-lower-angle", -0.7, 0.7, 0], ["neck-mid-angle", -0.8, 0.8, 0],
      ["neck-upper-angle", -0.9, 0.9, 0], ["head-yaw", -0.8, 0.8, 0],
      ["head-pitch", -0.7, 0.7, 0], ["head-roll", -0.55, 0.55, 0],
      ["chest-expand", -1, 1, 0], ["belly-compress", -1, 1, 0],
      ["body-lean-x", -2, 2, 0], ["body-lift", -10, 10, 0],
    ],
    face:[
      ["jaw-open", 0, 1, 0], ["muzzle-lift", -1, 1, 0],
      ["nose-twitch", -1, 1, 0], ["face-turn", -1, 1, 0],
      ["eye-left-look-x", -1, 1, 0], ["eye-left-look-y", -1, 1, 0],
      ["eye-right-look-x", -1, 1, 0], ["eye-right-look-y", -1, 1, 0],
      ["lid-left-upper", 0, 1, 0], ["lid-left-lower", 0, 1, 0],
      ["lid-right-upper", 0, 1, 0], ["lid-right-lower", 0, 1, 0],
      ["brow-left-lift", -1, 1, 0], ["brow-right-lift", -1, 1, 0],
      ["cheek-left-puff", 0, 1, 0], ["cheek-right-puff", 0, 1, 0],
    ],
    limbs:[
      ["front-left-shoulder-angle", -PI, PI, 0], ["front-left-elbow-angle", -PI, PI, 0],
      ["front-left-wrist-angle", -1.5, 1.5, 0], ["front-left-palm-angle", -1.2, 1.2, 0],
      ["front-left-reach-x", -18, 18, 0], ["front-left-reach-y", -18, 18, 0],
      ["front-left-lift", -12, 12, 0], ["front-left-twist", -0.8, 0.8, 0],
      ["front-right-shoulder-angle", -PI, PI, 0], ["front-right-elbow-angle", -PI, PI, 0],
      ["front-right-wrist-angle", -1.5, 1.5, 0], ["front-right-palm-angle", -1.2, 1.2, 0],
      ["front-right-reach-x", -18, 18, 0], ["front-right-reach-y", -18, 18, 0],
      ["front-right-lift", -12, 12, 0], ["front-right-twist", -0.8, 0.8, 0],
      ["rear-left-shoulder-angle", -PI, PI, 0], ["rear-left-elbow-angle", -PI, PI, 0],
      ["rear-left-wrist-angle", -1.5, 1.5, 0], ["rear-left-palm-angle", -1.2, 1.2, 0],
      ["rear-left-reach-x", -18, 18, 0], ["rear-left-reach-y", -18, 18, 0],
      ["rear-left-lift", -12, 12, 0], ["rear-left-twist", -0.8, 0.8, 0],
      ["rear-right-shoulder-angle", -PI, PI, 0], ["rear-right-elbow-angle", -PI, PI, 0],
      ["rear-right-wrist-angle", -1.5, 1.5, 0], ["rear-right-palm-angle", -1.2, 1.2, 0],
      ["rear-right-reach-x", -18, 18, 0], ["rear-right-reach-y", -18, 18, 0],
      ["rear-right-lift", -12, 12, 0], ["rear-right-twist", -0.8, 0.8, 0],
    ],
    claws:[
      ["claw-front-left-1-curl", 0, 1, 0.45], ["claw-front-left-1-spread", -1, 1, 0],
      ["claw-front-left-2-curl", 0, 1, 0.45], ["claw-front-left-2-spread", -1, 1, 0],
      ["claw-front-left-3-curl", 0, 1, 0.45], ["claw-front-left-3-spread", -1, 1, 0],
      ["claw-front-right-1-curl", 0, 1, 0.45], ["claw-front-right-1-spread", -1, 1, 0],
      ["claw-front-right-2-curl", 0, 1, 0.45], ["claw-front-right-2-spread", -1, 1, 0],
      ["claw-front-right-3-curl", 0, 1, 0.45], ["claw-front-right-3-spread", -1, 1, 0],
      ["claw-rear-left-1-curl", 0, 1, 0.45], ["claw-rear-left-1-spread", -1, 1, 0],
      ["claw-rear-left-2-curl", 0, 1, 0.45], ["claw-rear-left-2-spread", -1, 1, 0],
      ["claw-rear-left-3-curl", 0, 1, 0.45], ["claw-rear-left-3-spread", -1, 1, 0],
      ["claw-rear-right-1-curl", 0, 1, 0.45], ["claw-rear-right-1-spread", -1, 1, 0],
      ["claw-rear-right-2-curl", 0, 1, 0.45], ["claw-rear-right-2-spread", -1, 1, 0],
      ["claw-rear-right-3-curl", 0, 1, 0.45], ["claw-rear-right-3-spread", -1, 1, 0],
    ],
    fur:[
      ["fur-head-crest", -1, 1, 0], ["fur-head-cheek-left", -1, 1, 0],
      ["fur-head-cheek-right", -1, 1, 0], ["fur-head-nape", -1, 1, 0],
      ["fur-neck-ruff-left", -1, 1, 0], ["fur-neck-ruff-right", -1, 1, 0],
      ["fur-back-shoulder", -1, 1, 0], ["fur-back-mid", -1, 1, 0],
      ["fur-back-rump", -1, 1, 0], ["fur-belly-chest", -1, 1, 0],
      ["fur-belly-mid", -1, 1, 0], ["fur-belly-flank", -1, 1, 0],
    ],
    cable:[
      ["cable-sway", -12, 12, 0], ["cable-lift", -10, 10, 0],
      ["cable-tension", 0.85, 1, 1], ["cable-damping", 0.96, 0.999, 0.995],
      ["cable-wind", -1, 1, 0], ["cable-pulse", -1, 1, 0],
      ["cable-contact-bias", -1, 1, 0], ["cable-load-scale", 0.5, 1.5, 1],
    ],
    light:[
      ["light-key-intensity", 0, 1, 0.75], ["light-fill-intensity", 0, 1, 0.35],
      ["light-rim-intensity", 0, 1, 0.45], ["light-loaded-pulse", 0, 1, 0],
      ["light-searching-pulse", 0, 1, 0], ["light-visor-glow", 0, 1, 0],
    ],
    props:[
      ["prop-corona-tilt", -0.8, 0.8, 0], ["prop-casco-tilt", -0.8, 0.8, 0],
      ["prop-visor-open", 0, 1, 0], ["prop-fuego-height", 0, 1, 0.5],
      ["prop-bufanda-sway", -1, 1, 0], ["prop-huevo-wobble", -1, 1, 0],
    ],
  };

  const CHANNEL_GROUPS = {};
  const channelList = [];
  const limits = {};
  const channelIndexes = Object.create(null);
  for(const groupName of Object.keys(GROUP_SPECS)){
    const specs = GROUP_SPECS[groupName];
    const names = [];
    for(let index = 0; index < specs.length; index += 1){
      const spec = specs[index];
      const name = spec[0];
      if(channelIndexes[name] !== undefined) throw new Error(`duplicate PerezOS rig channel: ${name}`);
      channelIndexes[name] = channelList.length;
      channelList.push(name);
      names.push(name);
      limits[name] = Object.freeze({min:spec[1], max:spec[2], default:spec[3]});
    }
    CHANNEL_GROUPS[groupName] = Object.freeze(names);
  }
  const CHANNELS = Object.freeze(channelList);
  const LIMITS = Object.freeze(limits);
  Object.freeze(CHANNEL_GROUPS);
  if(CHANNELS.length !== 120) throw new Error(`PerezOS rig requires 120 channels, got ${CHANNELS.length}`);
  if(new Set(CHANNELS).size !== CHANNELS.length) throw new Error("PerezOS rig channel names must be unique");

  const PART_BY_ID = Object.create(null);
  for(let index = 0; index < Art.PARTS.length; index += 1){
    PART_BY_ID[Art.PARTS[index].id] = Art.PARTS[index];
  }
  for(let index = 0; index < LIMB_MANIFEST.length; index += 1){
    const item = LIMB_MANIFEST[index];
    if(!PART_BY_ID[item.upper] || !PART_BY_ID[item.lower] || !PART_BY_ID[item.palm]){
      throw new Error(`PerezOS art is missing the ${item.name} IK chain`);
    }
  }
  for(let limbIndex = 0; limbIndex < LIMB_NAMES.length; limbIndex += 1){
    for(let clawIndex = 1; clawIndex <= 3; clawIndex += 1){
      const id = `claw-${LIMB_NAMES[limbIndex]}-${clawIndex}`;
      if(!PART_BY_ID[id]) throw new Error(`PerezOS art is missing ${id}`);
    }
  }

  const CABLE_START_X = 25.89473684210526;
  const CABLE_START_Y = -11.277777777777779;
  const CABLE_END_X = 173.26315789473685;
  const CABLE_END_Y = 124.83333333333334;
  const CABLE_REST_X = Object.freeze([
    CABLE_START_X, 44.315789473684205, 62.73684210526316, 81.15789473684211,
    99.57894736842105, 140.5, 153, 154.8421052631579, CABLE_END_X,
  ]);
  const CABLE_REST_Y = Object.freeze(Array.from({length:CABLE_NODES},
    (_, node) => CABLE_START_Y + (CABLE_END_Y - CABLE_START_Y) *
      node / CABLE_SEGMENTS));
  const CABLE_REST_LENGTHS = Object.freeze(Array.from({length:CABLE_SEGMENTS},
    (_, segment) => Math.hypot(CABLE_REST_X[segment + 1] - CABLE_REST_X[segment],
      CABLE_REST_Y[segment + 1] - CABLE_REST_Y[segment])));

  const AXIAL_ZONES = Object.freeze(AXIAL_ZONE_IDS.map(id => {
    const bounds = PART_BY_ID[id].bounds;
    return Object.freeze({id, x:bounds[0], y:bounds[1],
      width:bounds[2], height:bounds[3]});
  }));

  const CLAW_IDS_BY_LIMB = Object.freeze(LIMB_NAMES.map(limb => Object.freeze([
    `claw-${limb}-1`, `claw-${limb}-2`, `claw-${limb}-3`,
  ])));
  const CLAW_IDS = Object.freeze(CLAW_IDS_BY_LIMB.flat());

  const BODY_LEAN_INDEX = channelIndexes["body-lean-x"];
  const BODY_LIFT_INDEX = channelIndexes["body-lift"];
  const CABLE_SWAY_INDEX = channelIndexes["cable-sway"];
  const CABLE_LIFT_INDEX = channelIndexes["cable-lift"];
  const CABLE_TENSION_INDEX = channelIndexes["cable-tension"];
  const CABLE_DAMPING_INDEX = channelIndexes["cable-damping"];
  const CABLE_WIND_INDEX = channelIndexes["cable-wind"];
  const CABLE_PULSE_INDEX = channelIndexes["cable-pulse"];
  const CABLE_BIAS_INDEX = channelIndexes["cable-contact-bias"];
  const CABLE_LOAD_INDEX = channelIndexes["cable-load-scale"];

  function anchorForPart(part){
    return Object.freeze({x:part.bounds[0] + part.pivot[0],
      y:part.bounds[1] + part.pivot[1]});
  }

  const LIMB_CONFIGS = Object.freeze(LIMB_MANIFEST.map(item => {
    const upperPart = PART_BY_ID[item.upper];
    const lowerPart = PART_BY_ID[item.lower];
    const palmPart = PART_BY_ID[item.palm];
    const restRoot = anchorForPart(upperPart);
    const restJoint = anchorForPart(lowerPart);
    const restEnd = anchorForPart(palmPart);
    const upperX = restJoint.x - restRoot.x;
    const upperY = restJoint.y - restRoot.y;
    const endX = restEnd.x - restRoot.x;
    const endY = restEnd.y - restRoot.y;
    const cross = upperX * endY - upperY * endX;
    if(Math.abs(cross) < 1e-9) throw new Error(`PerezOS ${item.name} IK chain has no bend branch`);
    return Object.freeze({
      name:item.name,
      upperPart:item.upper,
      lowerPart:item.lower,
      palmPart:item.palm,
      attachmentPart:upperPart.parent,
      restRoot,
      restJoint,
      restEnd,
      upperLength:Math.hypot(upperX, upperY),
      lowerLength:Math.hypot(restEnd.x - restJoint.x, restEnd.y - restJoint.y),
      bend:-Math.sign(cross),
      reachXIndex:channelIndexes[`${item.name}-reach-x`],
      reachYIndex:channelIndexes[`${item.name}-reach-y`],
      liftIndex:channelIndexes[`${item.name}-lift`],
    });
  }));

  function channelIndex(name){
    const index = channelIndexes[name];
    return index === undefined ? -1 : index;
  }

  function lockFields(record, fields){
    for(let index = 0; index < fields.length; index += 1){
      Object.defineProperty(record, fields[index], {writable:false});
    }
    return record;
  }

  function createPoint(x, y){
    return Object.seal({x, y});
  }

  function createSupport(limb, mode, cableT, load, x, y){
    const support = {limb, mode, cableT, load, point:createPoint(x, y),
      normal:createPoint(0, -1)};
    lockFields(support, ["limb", "point", "normal"]);
    return Object.seal(support);
  }

  function createSupports(){
    return Object.freeze({
      "front-left":createSupport("front-left", "loaded", 0.34, 0.58, 76, 35),
      "front-right":createSupport("front-right", "released", 0.55, 0, 0, 0),
      "rear-left":createSupport("rear-left", "released", 0.62, 0, 0, 0),
      "rear-right":createSupport("rear-right", "loaded", 0.72, 0.42, 150, 86.72222222222223),
    });
  }

  function createLimb(config){
    const limb = {
      name:config.name,
      upperPart:config.upperPart,
      lowerPart:config.lowerPart,
      palmPart:config.palmPart,
      restRoot:Object.freeze({x:config.restRoot.x, y:config.restRoot.y}),
      restJoint:Object.freeze({x:config.restJoint.x, y:config.restJoint.y}),
      restEnd:Object.freeze({x:config.restEnd.x, y:config.restEnd.y}),
      root:createPoint(config.restRoot.x, config.restRoot.y),
      baseRootX:config.restRoot.x,
      baseRootY:config.restRoot.y,
      restX:config.restEnd.x,
      restY:config.restEnd.y,
      upperLength:config.upperLength,
      lowerLength:config.lowerLength,
      bend:config.bend,
      reachXIndex:config.reachXIndex,
      reachYIndex:config.reachYIndex,
      liftIndex:config.liftIndex,
      target:createPoint(config.restEnd.x, config.restEnd.y),
      joint:createPoint(config.restJoint.x, config.restJoint.y),
      end:createPoint(config.restEnd.x, config.restEnd.y),
      upperAngle:Math.atan2(config.restJoint.y - config.restRoot.y,
        config.restJoint.x - config.restRoot.x),
      lowerAngle:Math.atan2(config.restJoint.y - config.restRoot.y,
        config.restJoint.x - config.restRoot.x) -
        Math.atan2(config.restEnd.y - config.restJoint.y,
          config.restEnd.x - config.restJoint.x),
      contactError:0,
    };
    lockFields(limb, [
      "name", "upperPart", "lowerPart", "palmPart", "restRoot", "restJoint", "restEnd",
      "root", "baseRootX", "baseRootY", "restX", "restY", "upperLength",
      "lowerLength", "bend", "reachXIndex", "reachYIndex", "liftIndex", "target",
      "joint", "end",
    ]);
    return Object.seal(limb);
  }

  function createLimbs(){
    const limbs = {};
    for(let index = 0; index < LIMB_CONFIGS.length; index += 1){
      const config = LIMB_CONFIGS[index];
      limbs[config.name] = createLimb(config);
    }
    return Object.freeze(limbs);
  }

  function createClaws(){
    const claws = {};
    for(let limbIndex = 0; limbIndex < LIMB_NAMES.length; limbIndex += 1){
      const limb = LIMB_NAMES[limbIndex];
      const ids = CLAW_IDS_BY_LIMB[limbIndex];
      for(let clawIndex = 0; clawIndex < ids.length; clawIndex += 1){
        const id = ids[clawIndex];
        const claw = {id, limb, index:clawIndex + 1, mode:"released", cableT:0,
          point:createPoint(0, 0), contactError:0};
        lockFields(claw, ["id", "limb", "index", "point"]);
        claws[id] = Object.seal(claw);
      }
    }
    return Object.freeze(claws);
  }

  function createInternalReferences(rig){
    const supportRefs = [];
    const supportPoints = [];
    const supportNormals = [];
    const limbRefs = [];
    const limbRestRoots = [];
    const limbRestJoints = [];
    const limbRestEnds = [];
    const limbRoots = [];
    const limbTargets = [];
    const limbJoints = [];
    const limbEnds = [];
    const clawGroups = [];
    const clawList = [];
    const clawPoints = [];
    for(let limbIndex = 0; limbIndex < LIMB_NAMES.length; limbIndex += 1){
      const name = LIMB_NAMES[limbIndex];
      supportRefs.push(rig.supports[name]);
      limbRefs.push(rig.limbs[name]);
      supportPoints.push(rig.supports[name].point);
      supportNormals.push(rig.supports[name].normal);
      limbRestRoots.push(rig.limbs[name].restRoot);
      limbRestJoints.push(rig.limbs[name].restJoint);
      limbRestEnds.push(rig.limbs[name].restEnd);
      limbRoots.push(rig.limbs[name].root);
      limbTargets.push(rig.limbs[name].target);
      limbJoints.push(rig.limbs[name].joint);
      limbEnds.push(rig.limbs[name].end);
      const ids = CLAW_IDS_BY_LIMB[limbIndex];
      const group = [];
      for(let clawIndex = 0; clawIndex < ids.length; clawIndex += 1){
        const claw = rig.claws[ids[clawIndex]];
        group.push(claw);
        clawList.push(claw);
        clawPoints.push(claw.point);
      }
      clawGroups.push(Object.freeze(group));
    }
    Object.freeze(clawGroups);
    Object.freeze(clawList);
    const buffers = Object.freeze({
      values:rig.values,
      targets:rig.targets,
      velocities:rig.velocities,
      lastValidValues:rig.lastValidValues,
      lastValidTargets:rig.lastValidTargets,
      lastValidVelocities:rig.lastValidVelocities,
      cable:rig.cable,
      cablePrevious:rig.cablePrevious,
      cableRestLengths:rig.cableRestLengths,
      lastValidCable:rig.lastValidCable,
      lastValidCablePrevious:rig.lastValidCablePrevious,
      lastValidSupports:rig.lastValidSupports,
      lastValidLimbs:rig.lastValidLimbs,
      lastValidClaws:rig.lastValidClaws,
    });
    const snapshot = {
      values:new Float64Array(CHANNELS.length),
      targets:new Float64Array(CHANNELS.length),
      velocities:new Float64Array(CHANNELS.length),
      cable:new Float64Array(CABLE_NODES * 2),
      cablePrevious:new Float64Array(CABLE_NODES * 2),
      supports:new Float64Array(LIMB_NAMES.length * SUPPORT_STRIDE),
      limbs:new Float64Array(LIMB_NAMES.length * LIMB_STRIDE),
      claws:new Float64Array(12 * CLAW_STRIDE),
      transferGoal:-1,
      lastDt:rig.lastDt,
      steps:0,
      maxCableStretch:0,
      maxLoadedContactError:0,
      cableEnergy:0,
    };
    const internal = {
      supportsContainer:rig.supports,
      limbsContainer:rig.limbs,
      clawsContainer:rig.claws,
      diagnostics:rig.diagnostics,
      buffers,
      snapshot,
      supportRefs,
      supportPoints,
      supportNormals,
      limbRefs,
      limbRestRoots,
      limbRestJoints,
      limbRestEnds,
      limbRoots,
      limbTargets,
      limbJoints,
      limbEnds,
      clawGroups,
      clawList,
      clawPoints,
      gripPoint:{x:0, y:0},
      recoveryCount:rig.diagnostics.recoveries,
    };
    rig.clawGroups = clawGroups;
    rig.clawList = clawList;
    RIG_INTERNALS.set(rig, internal);
    return internal;
  }

  function hardenRig(rig){
    for(let index = 0; index < PUBLIC_BUFFER_FIELDS.length; index += 1){
      Object.preventExtensions(rig[PUBLIC_BUFFER_FIELDS[index]]);
    }
    Object.seal(rig.diagnostics);
    lockFields(rig, RIG_STABLE_FIELDS);
    Object.seal(rig);
  }

  function initialiseCable(rig){
    for(let node = 0; node < CABLE_NODES; node += 1){
      const offset = node * 2;
      rig.cable[offset] = CABLE_REST_X[node];
      rig.cable[offset + 1] = CABLE_REST_Y[node];
      rig.cablePrevious[offset] = rig.cable[offset];
      rig.cablePrevious[offset + 1] = rig.cable[offset + 1];
    }
    for(let segment = 0; segment < CABLE_SEGMENTS; segment += 1){
      rig.cableRestLengths[segment] = CABLE_REST_LENGTHS[segment];
    }
  }

  function cablePoint(rig, t, output){
    const biased = Core.clamp(t + rig.values[CABLE_BIAS_INDEX] * 0.02, 0, 1);
    const scaled = biased * CABLE_SEGMENTS;
    let node = Math.floor(scaled);
    if(node >= CABLE_SEGMENTS) node = CABLE_SEGMENTS - 1;
    const fraction = scaled - node;
    const offset = node * 2;
    output.x = rig.cable[offset] + (rig.cable[offset + 2] - rig.cable[offset]) * fraction;
    output.y = rig.cable[offset + 1] +
      (rig.cable[offset + 3] - rig.cable[offset + 1]) * fraction;
  }

  function solveLimb(rig, limb, support, config){
    limb.root.x = config.restRoot.x + rig.values[BODY_LEAN_INDEX];
    limb.root.y = config.restRoot.y - rig.values[BODY_LIFT_INDEX];
    if(support.mode === "loaded"){
      limb.target.x = support.point.x;
      limb.target.y = support.point.y;
    }else{
      limb.target.x = config.restEnd.x + rig.values[config.reachXIndex];
      limb.target.y = config.restEnd.y + rig.values[config.reachYIndex] -
        rig.values[config.liftIndex];
    }

    const dx = limb.target.x - limb.root.x;
    const dy = limb.target.y - limb.root.y;
    const lowerBound = Math.abs(config.upperLength - config.lowerLength) + 0.001;
    const upperBound = config.upperLength + config.lowerLength - 0.001;
    const rawDistance = Math.hypot(dx, dy);
    const distance = Core.clamp(rawDistance, lowerBound, upperBound);
    const base = Math.atan2(dy, dx);
    const shoulder = Math.acos(Core.clamp(
      (config.upperLength * config.upperLength + distance * distance -
       config.lowerLength * config.lowerLength) / (2 * config.upperLength * distance), -1, 1));
    const elbow = Math.acos(Core.clamp(
      (config.upperLength * config.upperLength + config.lowerLength * config.lowerLength -
       distance * distance) / (2 * config.upperLength * config.lowerLength), -1, 1));

    limb.upperAngle = base + config.bend * shoulder;
    limb.lowerAngle = config.bend * (PI - elbow);
    limb.joint.x = limb.root.x + Math.cos(limb.upperAngle) * config.upperLength;
    limb.joint.y = limb.root.y + Math.sin(limb.upperAngle) * config.upperLength;
    const lowerWorldAngle = limb.upperAngle - limb.lowerAngle;
    limb.end.x = limb.joint.x + Math.cos(lowerWorldAngle) * config.lowerLength;
    limb.end.y = limb.joint.y + Math.sin(lowerWorldAngle) * config.lowerLength;
    limb.contactError = support.mode === "loaded" ?
      Math.hypot(limb.end.x - support.point.x, limb.end.y - support.point.y) : 0;
  }

  function solveAllLimbs(rig){
    const internal = RIG_INTERNALS.get(rig);
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      solveLimb(rig, internal.limbRefs[index], internal.supportRefs[index],
        LIMB_CONFIGS[index]);
    }
  }

  function updateClaws(rig){
    const internal = RIG_INTERNALS.get(rig);
    for(let limbIndex = 0; limbIndex < LIMB_NAMES.length; limbIndex += 1){
      const support = internal.supportRefs[limbIndex];
      const limb = internal.limbRefs[limbIndex];
      const claws = internal.clawGroups[limbIndex];
      for(let clawIndex = 0; clawIndex < claws.length; clawIndex += 1){
        const claw = claws[clawIndex];
        claw.mode = support.mode;
        claw.cableT = support.cableT;
        claw.point.x = limb.end.x;
        claw.point.y = limb.end.y;
        claw.contactError = support.mode === "loaded" ?
          Math.hypot(claw.point.x - support.point.x, claw.point.y - support.point.y) : 0;
      }
    }
  }

  function pinCable(rig){
    const sway = rig.values[CABLE_SWAY_INDEX];
    const lift = rig.values[CABLE_LIFT_INDEX];
    rig.cable[0] = CABLE_START_X + sway;
    rig.cable[1] = CABLE_START_Y - lift;
    rig.cable[CABLE_SEGMENTS * 2] = CABLE_END_X + sway;
    rig.cable[CABLE_SEGMENTS * 2 + 1] = CABLE_END_Y - lift;
  }

  function integrateCable(rig, dt){
    const supports = RIG_INTERNALS.get(rig).supportRefs;
    const damping = rig.values[CABLE_DAMPING_INDEX];
    const wind = rig.values[CABLE_WIND_INDEX] * 24;
    const pulse = rig.values[CABLE_PULSE_INDEX] * 9;
    const loadScale = rig.values[CABLE_LOAD_INDEX];
    const dtSquared = dt * dt;
    for(let node = 1; node < CABLE_SEGMENTS; node += 1){
      const offset = node * 2;
      const x = rig.cable[offset];
      const y = rig.cable[offset + 1];
      const velocityX = (x - rig.cablePrevious[offset]) * damping;
      const velocityY = (y - rig.cablePrevious[offset + 1]) * damping;
      let bodyLoad = 0;
      const nodeT = node / CABLE_SEGMENTS;
      for(let supportIndex = 0; supportIndex < LIMB_NAMES.length; supportIndex += 1){
        const support = supports[supportIndex];
        const weight = Math.max(0, 1 - Math.abs(nodeT - support.cableT) * CABLE_SEGMENTS);
        bodyLoad += support.load * weight;
      }
      rig.cablePrevious[offset] = x;
      rig.cablePrevious[offset + 1] = y;
      rig.cable[offset] = x + velocityX + (wind + pulse * (nodeT - 0.5)) * dtSquared;
      rig.cable[offset + 1] = y + velocityY + (10 + bodyLoad * 34 * loadScale) * dtSquared;
    }

    pinCable(rig);
    const tension = rig.values[CABLE_TENSION_INDEX];
    for(let iteration = 0; iteration < CABLE_ITERATIONS; iteration += 1){
      for(let segment = 0; segment < CABLE_SEGMENTS; segment += 1){
        const left = segment * 2;
        const right = left + 2;
        const dx = rig.cable[right] - rig.cable[left];
        const dy = rig.cable[right + 1] - rig.cable[left + 1];
        const distance = Math.hypot(dx, dy);
        if(distance <= 1e-12) continue;
        const correction = (distance - CABLE_REST_LENGTHS[segment]) / distance * tension;
        if(segment === 0){
          rig.cable[right] -= dx * correction;
          rig.cable[right + 1] -= dy * correction;
        }else if(segment === CABLE_SEGMENTS - 1){
          rig.cable[left] += dx * correction;
          rig.cable[left + 1] += dy * correction;
        }else{
          const half = correction * 0.5;
          rig.cable[left] += dx * half;
          rig.cable[left + 1] += dy * half;
          rig.cable[right] -= dx * half;
          rig.cable[right + 1] -= dy * half;
        }
      }
      pinCable(rig);
    }
  }

  function updateSupportTransfer(rig, dt){
    const supports = RIG_INTERNALS.get(rig).supportRefs;
    const goalIndex = rig.transferGoal;
    if(goalIndex < 0) return;
    const goal = supports[goalIndex];
    if(goal.mode !== "loaded") return;
    const missing = 1 - goal.load;
    if(missing <= 1e-12){
      goal.load = 1;
      return;
    }
    const moved = Math.min(missing, dt * 1.25);
    let donorTotal = 0;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      if(index !== goalIndex) donorTotal += supports[index].load;
    }
    if(donorTotal <= 0) return;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      if(index === goalIndex) continue;
      const support = supports[index];
      support.load -= moved * support.load / donorTotal;
      if(support.load < 1e-12) support.load = 0;
    }
    goal.load += moved;
    if(goal.load > 1 - 1e-12) goal.load = 1;
  }

  function updateChannels(rig, dt){
    for(let index = 0; index < CHANNELS.length; index += 1){
      const limit = LIMITS[CHANNELS[index]];
      const target = Core.clamp(rig.targets[index], limit.min, limit.max);
      rig.targets[index] = target;
      const acceleration = (target - rig.values[index]) * 72 - rig.velocities[index] * 17;
      rig.velocities[index] += acceleration * dt;
      rig.values[index] += rig.velocities[index] * dt;
      if(rig.values[index] < limit.min){
        rig.values[index] = limit.min;
        if(rig.velocities[index] < 0) rig.velocities[index] = 0;
      }else if(rig.values[index] > limit.max){
        rig.values[index] = limit.max;
        if(rig.velocities[index] > 0) rig.velocities[index] = 0;
      }
    }
  }

  function updateSupportPoints(rig){
    const supports = RIG_INTERNALS.get(rig).supportRefs;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = supports[index];
      if(support.mode === "loaded") cablePoint(rig, support.cableT, support.point);
    }
  }

  function computeMetrics(rig, dt){
    const internal = RIG_INTERNALS.get(rig);
    let maxStretch = 0;
    let energy = 0;
    const inverseDt = dt > 0 ? 1 / dt : 0;
    for(let segment = 0; segment < CABLE_SEGMENTS; segment += 1){
      const offset = segment * 2;
      const distance = Math.hypot(rig.cable[offset + 2] - rig.cable[offset],
        rig.cable[offset + 3] - rig.cable[offset + 1]);
      const stretch = Math.abs(distance - CABLE_REST_LENGTHS[segment]) /
        CABLE_REST_LENGTHS[segment];
      if(stretch > maxStretch) maxStretch = stretch;
    }
    for(let node = 0; node < CABLE_NODES; node += 1){
      const offset = node * 2;
      const velocityX = (rig.cable[offset] - rig.cablePrevious[offset]) * inverseDt;
      const velocityY = (rig.cable[offset + 1] - rig.cablePrevious[offset + 1]) * inverseDt;
      energy += 0.5 * (velocityX * velocityX + velocityY * velocityY) +
        10 * (Art.WORLD.height - rig.cable[offset + 1]);
    }
    let maxContactError = 0;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = internal.supportRefs[index];
      if(support.mode === "loaded"){
        const claws = internal.clawGroups[index];
        for(let clawIndex = 0; clawIndex < claws.length; clawIndex += 1){
          const claw = claws[clawIndex];
          const error = Math.hypot(claw.point.x - support.point.x,
            claw.point.y - support.point.y);
          if(error > maxContactError) maxContactError = error;
        }
      }
    }
    rig.diagnostics.maxCableStretch = maxStretch;
    rig.diagnostics.maxLoadedContactError = maxContactError;
    rig.diagnostics.cableEnergy = energy;
  }

  function pointInZone(x, y, zone, lean, lift){
    const minX = zone.x + lean;
    const minY = zone.y - lift;
    return x >= minX && x <= minX + zone.width &&
      y >= minY && y <= minY + zone.height;
  }

  function segmentIntersectsZone(x1, y1, x2, y2, zone, lean, lift){
    const minX = zone.x + lean;
    const maxX = minX + zone.width;
    const minY = zone.y - lift;
    const maxY = minY + zone.height;
    const dx = x2 - x1;
    const dy = y2 - y1;
    let enter = 0;
    let exit = 1;
    if(dx === 0){
      if(x1 < minX || x1 > maxX) return false;
    }else{
      let first = (minX - x1) / dx;
      let second = (maxX - x1) / dx;
      if(first > second){
        const swap = first;
        first = second;
        second = swap;
      }
      if(first > enter) enter = first;
      if(second < exit) exit = second;
      if(enter > exit) return false;
    }
    if(dy === 0){
      if(y1 < minY || y1 > maxY) return false;
    }else{
      let first = (minY - y1) / dy;
      let second = (maxY - y1) / dy;
      if(first > second){
        const swap = first;
        first = second;
        second = swap;
      }
      if(first > enter) enter = first;
      if(second < exit) exit = second;
      if(enter > exit) return false;
    }
    return exit >= 0 && enter <= 1;
  }

  function bodyOverlapKind(rig, limb, support, config, claws){
    const lean = rig.values[BODY_LEAN_INDEX];
    const lift = rig.values[BODY_LIFT_INDEX];
    for(let zoneIndex = 0; zoneIndex < AXIAL_ZONES.length; zoneIndex += 1){
      const zone = AXIAL_ZONES[zoneIndex];
      if(pointInZone(limb.joint.x, limb.joint.y, zone, lean, lift) ||
         pointInZone(limb.end.x, limb.end.y, zone, lean, lift)) return 1;

      const rootAttachedInside = zone.id === config.attachmentPart &&
        pointInZone(limb.root.x, limb.root.y, zone, lean, lift);
      if(segmentIntersectsZone(limb.root.x, limb.root.y, limb.joint.x, limb.joint.y,
          zone, lean, lift) &&
         !(rootAttachedInside &&
           !pointInZone(limb.joint.x, limb.joint.y, zone, lean, lift))) return 2;
      if(segmentIntersectsZone(limb.joint.x, limb.joint.y, limb.end.x, limb.end.y,
          zone, lean, lift)) return 2;

      if(support.mode === "loaded" &&
         pointInZone(support.point.x, support.point.y, zone, lean, lift)) return 3;
      if(support.mode === "loaded"){
        for(let clawIndex = 0; clawIndex < claws.length; clawIndex += 1){
          const claw = claws[clawIndex];
          const point = claw && claw.point;
          if(!point || !Number.isFinite(point.x) || !Number.isFinite(point.y)) return 3;
          if(pointInZone(point.x, point.y, zone, lean, lift)) return 3;
        }
      }
    }
    return 0;
  }

  function limbStructureMatches(limb, config){
    return limb && limb.name === config.name && limb.upperPart === config.upperPart &&
      limb.lowerPart === config.lowerPart && limb.palmPart === config.palmPart &&
      limb.restRoot && limb.restRoot.x === config.restRoot.x &&
      limb.restRoot.y === config.restRoot.y && limb.restJoint &&
      limb.restJoint.x === config.restJoint.x && limb.restJoint.y === config.restJoint.y &&
      limb.restEnd && limb.restEnd.x === config.restEnd.x &&
      limb.restEnd.y === config.restEnd.y && limb.baseRootX === config.restRoot.x &&
      limb.baseRootY === config.restRoot.y && limb.restX === config.restEnd.x &&
      limb.restY === config.restEnd.y && limb.upperLength === config.upperLength &&
      limb.lowerLength === config.lowerLength && limb.bend === config.bend &&
      limb.reachXIndex === config.reachXIndex && limb.reachYIndex === config.reachYIndex &&
      limb.liftIndex === config.liftIndex;
  }

  function limbTargetMatches(rig, limb, support, config){
    const expectedX = support.mode === "loaded" ? support.point.x :
      config.restEnd.x + rig.values[config.reachXIndex];
    const expectedY = support.mode === "loaded" ? support.point.y :
      config.restEnd.y + rig.values[config.reachYIndex] - rig.values[config.liftIndex];
    return Math.abs(limb.target.x - expectedX) <= 1e-9 &&
      Math.abs(limb.target.y - expectedY) <= 1e-9;
  }

  function limbAnglesMatch(limb){
    const upper = Math.atan2(limb.joint.y - limb.root.y, limb.joint.x - limb.root.x);
    const lowerWorld = Math.atan2(limb.end.y - limb.joint.y, limb.end.x - limb.joint.x);
    const lower = upper - lowerWorld;
    return Math.abs(Math.atan2(Math.sin(limb.upperAngle - upper),
      Math.cos(limb.upperAngle - upper))) <= 1e-9 &&
      Math.abs(Math.atan2(Math.sin(limb.lowerAngle - lower),
      Math.cos(limb.lowerAngle - lower))) <= 1e-9;
  }

  function pointMatchesBranch(limb, point, config){
    const upperX = limb.joint.x - limb.root.x;
    const upperY = limb.joint.y - limb.root.y;
    const pointX = point.x - limb.root.x;
    const pointY = point.y - limb.root.y;
    const scale = Math.hypot(upperX, upperY) * Math.hypot(pointX, pointY);
    if(!Number.isFinite(scale) || scale <= 1e-12) return false;
    const normalizedCross = (upperX * pointY - upperY * pointX) / scale;
    return Number.isFinite(normalizedCross) &&
      normalizedCross * -config.bend > 1e-8;
  }

  function limbBranchMatches(limb, config){
    return pointMatchesBranch(limb, limb.end, config) &&
      pointMatchesBranch(limb, limb.target, config);
  }

  function buffersMatch(left, right){
    if(!left || !right || left.length !== right.length) return false;
    for(let index = 0; index < left.length; index += 1){
      if(left[index] !== right[index]) return false;
    }
    return true;
  }

  function runtimeTopologyValid(rig, internal){
    if(!internal || !Object.isSealed(rig) || rig.clawGroups !== internal.clawGroups ||
       rig.clawList !== internal.clawList || !Object.isFrozen(rig.clawGroups) ||
       !Object.isFrozen(rig.clawList) || rig.supports !== internal.supportsContainer ||
       rig.limbs !== internal.limbsContainer || rig.claws !== internal.clawsContainer ||
       !Object.isFrozen(rig.supports) || !Object.isFrozen(rig.limbs) ||
       !Object.isFrozen(rig.claws) || rig.diagnostics !== internal.diagnostics ||
       !Object.isSealed(rig.diagnostics)) return false;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const name = LIMB_NAMES[index];
      const support = internal.supportRefs[index];
      const limb = internal.limbRefs[index];
      if(rig.supports[name] !== support || rig.limbs[name] !== limb ||
         !support || !Object.isSealed(support) ||
         support.point !== internal.supportPoints[index] ||
         support.normal !== internal.supportNormals[index] || !Object.isSealed(support.point) ||
         !Object.isSealed(support.normal) ||
         !limb || !Object.isSealed(limb) || limb.restRoot !== internal.limbRestRoots[index] ||
         limb.restJoint !== internal.limbRestJoints[index] ||
         limb.restEnd !== internal.limbRestEnds[index] || limb.root !== internal.limbRoots[index] ||
         limb.target !== internal.limbTargets[index] || limb.joint !== internal.limbJoints[index] ||
         limb.end !== internal.limbEnds[index] || !Object.isFrozen(limb.restRoot) ||
         !Object.isFrozen(limb.restJoint) || !Object.isFrozen(limb.restEnd) ||
         !Object.isSealed(limb.root) || !Object.isSealed(limb.target) ||
         !Object.isSealed(limb.joint) || !Object.isSealed(limb.end)) return false;
    }
    for(let index = 0; index < internal.clawList.length; index += 1){
      const claw = internal.clawList[index];
      if(rig.claws[CLAW_IDS[index]] !== claw || !claw || !Object.isSealed(claw) ||
         claw.point !== internal.clawPoints[index] || !Object.isSealed(claw.point)) return false;
    }
    return true;
  }

  function sameValueWritable(record, field){
    if(!record) return false;
    const value = record[field];
    try{
      record[field] = value;
    }catch(error){
      return false;
    }
    return Object.is(record[field], value);
  }

  function fieldsWritable(record, fields){
    for(let index = 0; index < fields.length; index += 1){
      if(!sameValueWritable(record, fields[index])) return false;
    }
    return true;
  }

  function transitionWritable(rig, internal){
    if(!runtimeTopologyValid(rig, internal) ||
       !fieldsWritable(rig, RIG_DYNAMIC_FIELDS) ||
       !fieldsWritable(internal.diagnostics, DIAGNOSTIC_DYNAMIC_FIELDS)) return false;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      if(!fieldsWritable(internal.supportRefs[index], SUPPORT_DYNAMIC_FIELDS) ||
         !fieldsWritable(internal.supportPoints[index], POINT_RESTORE_FIELDS) ||
         !fieldsWritable(internal.supportNormals[index], POINT_RESTORE_FIELDS) ||
         !fieldsWritable(internal.limbRefs[index], LIMB_DYNAMIC_FIELDS) ||
         !fieldsWritable(internal.limbRoots[index], POINT_RESTORE_FIELDS) ||
         !fieldsWritable(internal.limbTargets[index], POINT_RESTORE_FIELDS) ||
         !fieldsWritable(internal.limbJoints[index], POINT_RESTORE_FIELDS) ||
         !fieldsWritable(internal.limbEnds[index], POINT_RESTORE_FIELDS)) return false;
    }
    for(let index = 0; index < internal.clawList.length; index += 1){
      if(!fieldsWritable(internal.clawList[index], CLAW_DYNAMIC_FIELDS) ||
         !fieldsWritable(internal.clawPoints[index], POINT_RESTORE_FIELDS)) return false;
    }
    return true;
  }

  function internallyValid(rig, allowUnboundedTargets){
    const internal = RIG_INTERNALS.get(rig);
    const snapshot = internal && internal.snapshot;
    if(!internal || !transitionWritable(rig, internal) ||
       !authoritativeBuffersUsable(rig, internal) || !Number.isInteger(rig.transferGoal) ||
       rig.transferGoal < -1 || rig.transferGoal >= LIMB_NAMES.length ||
       (rig.transferGoal >= 0 && internal.supportRefs[rig.transferGoal].mode !== "loaded") ||
       rig.cableIterations !== CABLE_ITERATIONS ||
       !Number.isFinite(rig.lastDt) || rig.lastDt < 0 || rig.lastDt > MAX_DT) return false;
    if(rig.lastValidTransferGoal !== snapshot.transferGoal ||
       rig.lastValidDt !== snapshot.lastDt || rig.lastValidSteps !== snapshot.steps ||
       rig.lastValidMaxCableStretch !== snapshot.maxCableStretch ||
       rig.lastValidMaxLoadedContactError !== snapshot.maxLoadedContactError ||
       rig.lastValidCableEnergy !== snapshot.cableEnergy ||
       !buffersMatch(rig.lastValidValues, snapshot.values) ||
       !buffersMatch(rig.lastValidTargets, snapshot.targets) ||
       !buffersMatch(rig.lastValidVelocities, snapshot.velocities) ||
       !buffersMatch(rig.lastValidCable, snapshot.cable) ||
       !buffersMatch(rig.lastValidCablePrevious, snapshot.cablePrevious) ||
       !buffersMatch(rig.lastValidSupports, snapshot.supports) ||
       !buffersMatch(rig.lastValidLimbs, snapshot.limbs) ||
       !buffersMatch(rig.lastValidClaws, snapshot.claws)) return false;
    let supportLoad = 0;
    let loadedSupports = 0;
    for(let index = 0; index < CHANNELS.length; index += 1){
      const limit = LIMITS[CHANNELS[index]];
      if(!Number.isFinite(rig.values[index]) || rig.values[index] < limit.min - 1e-9 ||
         rig.values[index] > limit.max + 1e-9 || !Number.isFinite(rig.targets[index]) ||
         (!allowUnboundedTargets && (rig.targets[index] < limit.min - 1e-9 ||
          rig.targets[index] > limit.max + 1e-9)) ||
         !Number.isFinite(rig.velocities[index])) return false;
    }
    for(let index = 0; index < rig.cable.length; index += 1){
      if(!Number.isFinite(rig.cable[index]) || !Number.isFinite(rig.cablePrevious[index])) return false;
    }
    for(let segment = 0; segment < CABLE_SEGMENTS; segment += 1){
      if(rig.cableRestLengths[segment] !== CABLE_REST_LENGTHS[segment]) return false;
    }
    if(!Number.isInteger(rig.diagnostics.recoveries) || rig.diagnostics.recoveries < 0 ||
       rig.diagnostics.recoveries !== internal.recoveryCount ||
       !Number.isInteger(rig.diagnostics.steps) || rig.diagnostics.steps < 0 ||
       !Number.isFinite(rig.diagnostics.maxCableStretch) ||
       rig.diagnostics.maxCableStretch < 0 ||
       rig.diagnostics.maxCableStretch >= MAX_CABLE_STRETCH ||
       !Number.isFinite(rig.diagnostics.maxLoadedContactError) ||
       rig.diagnostics.maxLoadedContactError < 0 ||
       rig.diagnostics.maxLoadedContactError >= MAX_CONTACT_ERROR ||
       !Number.isFinite(rig.diagnostics.cableEnergy) || rig.diagnostics.cableEnergy < 0) return false;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const name = LIMB_NAMES[index];
      const support = internal.supportRefs[index];
      const limb = internal.limbRefs[index];
      const config = LIMB_CONFIGS[index];
      const claws = internal.clawGroups[index];
      if(rig.supports[name] !== support || rig.limbs[name] !== limb || !support ||
         !support.point || !support.normal || !limb || !limb.root || !limb.target ||
         !limb.joint || !limb.end || support.limb !== name ||
         !limbStructureMatches(limb, config) ||
         (support.mode !== "loaded" && support.mode !== "released") ||
         !Number.isFinite(support.cableT) || support.cableT < 0 || support.cableT > 1 ||
         !Number.isFinite(support.load) || support.load < -1e-9 || support.load > 1 + 1e-9 ||
         !Number.isFinite(support.point.x) ||
         !Number.isFinite(support.point.y) || !Number.isFinite(support.normal.x) ||
         !Number.isFinite(support.normal.y) || !Number.isFinite(limb.root.x) ||
         !Number.isFinite(limb.root.y) || !Number.isFinite(limb.target.x) ||
         !Number.isFinite(limb.target.y) || !Number.isFinite(limb.joint.x) ||
         !Number.isFinite(limb.joint.y) || !Number.isFinite(limb.end.x) ||
         !Number.isFinite(limb.end.y) || !Number.isFinite(limb.upperAngle) ||
         !Number.isFinite(limb.lowerAngle) || !Number.isFinite(limb.contactError)) return false;
      if(support.mode === "released" && support.load !== 0) return false;
      if(support.mode === "loaded") loadedSupports += 1;
      if(support.mode === "loaded"){
        cablePoint(rig, support.cableT, internal.gripPoint);
        if(Math.hypot(support.point.x - internal.gripPoint.x,
            support.point.y - internal.gripPoint.y) > 1e-9) return false;
      }
      if(Math.abs(limb.root.x - (config.restRoot.x + rig.values[BODY_LEAN_INDEX])) > 1e-9 ||
         Math.abs(limb.root.y - (config.restRoot.y - rig.values[BODY_LIFT_INDEX])) > 1e-9 ||
         !limbTargetMatches(rig, limb, support, config) || !limbAnglesMatch(limb) ||
         !limbBranchMatches(limb, config) ||
         Math.abs(Math.hypot(limb.joint.x - limb.root.x, limb.joint.y - limb.root.y) -
          config.upperLength) > 1e-5 ||
         Math.abs(Math.hypot(limb.end.x - limb.joint.x, limb.end.y - limb.joint.y) -
          config.lowerLength) > 1e-5 ||
         Math.abs(Math.hypot(support.normal.x, support.normal.y) - 1) > 1e-6) return false;
      if(bodyOverlapKind(rig, limb, support, config, claws) !== 0) return false;
      const limbError = support.mode === "loaded" ?
        Math.hypot(limb.end.x - support.point.x, limb.end.y - support.point.y) : 0;
      if(limbError >= MAX_CONTACT_ERROR ||
         Math.abs(limb.contactError - limbError) > 1e-9) return false;
      for(let clawIndex = 0; clawIndex < claws.length; clawIndex += 1){
        const claw = claws[clawIndex];
        const clawId = CLAW_IDS_BY_LIMB[index][clawIndex];
        if(rig.claws[clawId] !== claw || !claw || !claw.point || claw.id !== clawId ||
           claw.limb !== name ||
           claw.index !== clawIndex + 1 || claw.mode !== support.mode ||
           !Number.isFinite(claw.cableT) || Math.abs(claw.cableT - support.cableT) > 1e-12 ||
           !Number.isFinite(claw.point.x) || !Number.isFinite(claw.point.y) ||
           !Number.isFinite(claw.contactError)) return false;
        if(Math.hypot(claw.point.x - limb.end.x, claw.point.y - limb.end.y) > 1e-9) return false;
        const clawError = support.mode === "loaded" ?
          Math.hypot(claw.point.x - support.point.x, claw.point.y - support.point.y) : 0;
        if(clawError >= MAX_CONTACT_ERROR ||
           Math.abs(claw.contactError - clawError) > 1e-9) return false;
      }
      supportLoad += support.load;
    }
    if(loadedSupports < 1) return false;
    if(Math.abs(supportLoad - 1) >= 1e-5) return false;
    return true;
  }

  function saveLastValid(rig){
    const internal = RIG_INTERNALS.get(rig);
    internal.recoveryCount = rig.diagnostics.recoveries;
    rig.lastValidValues.set(rig.values);
    rig.lastValidTargets.set(rig.targets);
    rig.lastValidVelocities.set(rig.velocities);
    rig.lastValidCable.set(rig.cable);
    rig.lastValidCablePrevious.set(rig.cablePrevious);
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = internal.supportRefs[index];
      let offset = index * SUPPORT_STRIDE;
      rig.lastValidSupports[offset] = support.mode === "loaded" ? 1 : 0;
      rig.lastValidSupports[offset + 1] = support.cableT;
      rig.lastValidSupports[offset + 2] = support.load;
      rig.lastValidSupports[offset + 3] = support.point.x;
      rig.lastValidSupports[offset + 4] = support.point.y;
      rig.lastValidSupports[offset + 5] = support.normal.x;
      rig.lastValidSupports[offset + 6] = support.normal.y;

      const limb = internal.limbRefs[index];
      offset = index * LIMB_STRIDE;
      rig.lastValidLimbs[offset] = limb.target.x;
      rig.lastValidLimbs[offset + 1] = limb.target.y;
      rig.lastValidLimbs[offset + 2] = limb.joint.x;
      rig.lastValidLimbs[offset + 3] = limb.joint.y;
      rig.lastValidLimbs[offset + 4] = limb.end.x;
      rig.lastValidLimbs[offset + 5] = limb.end.y;
      rig.lastValidLimbs[offset + 6] = limb.upperAngle;
      rig.lastValidLimbs[offset + 7] = limb.lowerAngle;
      rig.lastValidLimbs[offset + 8] = limb.contactError;
    }
    let clawOffset = 0;
    for(let clawIndex = 0; clawIndex < internal.clawList.length; clawIndex += 1){
      const claw = internal.clawList[clawIndex];
      rig.lastValidClaws[clawOffset] = claw.mode === "loaded" ? 1 : 0;
      rig.lastValidClaws[clawOffset + 1] = claw.cableT;
      rig.lastValidClaws[clawOffset + 2] = claw.point.x;
      rig.lastValidClaws[clawOffset + 3] = claw.point.y;
      rig.lastValidClaws[clawOffset + 4] = claw.contactError;
      clawOffset += CLAW_STRIDE;
    }
    rig.lastValidTransferGoal = rig.transferGoal;
    rig.lastValidDt = rig.lastDt;
    rig.lastValidSteps = rig.diagnostics.steps;
    rig.lastValidMaxCableStretch = rig.diagnostics.maxCableStretch;
    rig.lastValidMaxLoadedContactError = rig.diagnostics.maxLoadedContactError;
    rig.lastValidCableEnergy = rig.diagnostics.cableEnergy;
    const snapshot = internal.snapshot;
    snapshot.values.set(rig.values);
    snapshot.targets.set(rig.targets);
    snapshot.velocities.set(rig.velocities);
    snapshot.cable.set(rig.cable);
    snapshot.cablePrevious.set(rig.cablePrevious);
    snapshot.supports.set(rig.lastValidSupports);
    snapshot.limbs.set(rig.lastValidLimbs);
    snapshot.claws.set(rig.lastValidClaws);
    snapshot.transferGoal = rig.transferGoal;
    snapshot.lastDt = rig.lastDt;
    snapshot.steps = rig.diagnostics.steps;
    snapshot.maxCableStretch = rig.diagnostics.maxCableStretch;
    snapshot.maxLoadedContactError = rig.diagnostics.maxLoadedContactError;
    snapshot.cableEnergy = rig.diagnostics.cableEnergy;
  }

  function expectedBuffer(buffer, length){
    return buffer instanceof Float64Array && buffer.length === length && Object.isSealed(buffer);
  }

  function authoritativeBuffersUsable(rig, internal){
    const buffers = internal && internal.buffers;
    return buffers && rig.values === buffers.values && rig.targets === buffers.targets &&
      rig.velocities === buffers.velocities &&
      rig.lastValidValues === buffers.lastValidValues &&
      rig.lastValidTargets === buffers.lastValidTargets &&
      rig.lastValidVelocities === buffers.lastValidVelocities &&
      rig.cable === buffers.cable && rig.cablePrevious === buffers.cablePrevious &&
      rig.cableRestLengths === buffers.cableRestLengths &&
      rig.lastValidCable === buffers.lastValidCable &&
      rig.lastValidCablePrevious === buffers.lastValidCablePrevious &&
      rig.lastValidSupports === buffers.lastValidSupports &&
      rig.lastValidLimbs === buffers.lastValidLimbs &&
      rig.lastValidClaws === buffers.lastValidClaws &&
      expectedBuffer(rig.values, CHANNELS.length) &&
      expectedBuffer(rig.targets, CHANNELS.length) &&
      expectedBuffer(rig.velocities, CHANNELS.length) &&
      expectedBuffer(rig.lastValidValues, CHANNELS.length) &&
      expectedBuffer(rig.lastValidTargets, CHANNELS.length) &&
      expectedBuffer(rig.lastValidVelocities, CHANNELS.length) &&
      expectedBuffer(rig.cable, CABLE_NODES * 2) &&
      expectedBuffer(rig.cablePrevious, CABLE_NODES * 2) &&
      expectedBuffer(rig.cableRestLengths, CABLE_SEGMENTS) &&
      expectedBuffer(rig.lastValidCable, CABLE_NODES * 2) &&
      expectedBuffer(rig.lastValidCablePrevious, CABLE_NODES * 2) &&
      expectedBuffer(rig.lastValidSupports, LIMB_NAMES.length * SUPPORT_STRIDE) &&
      expectedBuffer(rig.lastValidLimbs, LIMB_NAMES.length * LIMB_STRIDE) &&
      expectedBuffer(rig.lastValidClaws, CLAW_IDS.length * CLAW_STRIDE);
  }

  function restoreLastValid(rig){
    const internal = RIG_INTERNALS.get(rig);
    if(!internal || !transitionWritable(rig, internal) ||
       !authoritativeBuffersUsable(rig, internal)) return false;
    const snapshot = internal.snapshot;
    const recoveries = internal.recoveryCount;
    rig.values.set(snapshot.values);
    rig.targets.set(snapshot.targets);
    rig.velocities.set(snapshot.velocities);
    rig.cable.set(snapshot.cable);
    rig.cablePrevious.set(snapshot.cablePrevious);
    rig.lastValidValues.set(snapshot.values);
    rig.lastValidTargets.set(snapshot.targets);
    rig.lastValidVelocities.set(snapshot.velocities);
    rig.lastValidCable.set(snapshot.cable);
    rig.lastValidCablePrevious.set(snapshot.cablePrevious);
    rig.lastValidSupports.set(snapshot.supports);
    rig.lastValidLimbs.set(snapshot.limbs);
    rig.lastValidClaws.set(snapshot.claws);
    for(let segment = 0; segment < CABLE_SEGMENTS; segment += 1){
      rig.cableRestLengths[segment] = CABLE_REST_LENGTHS[segment];
    }
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = internal.supportRefs[index];
      let offset = index * SUPPORT_STRIDE;
      support.mode = snapshot.supports[offset] === 1 ? "loaded" : "released";
      support.cableT = snapshot.supports[offset + 1];
      support.load = snapshot.supports[offset + 2];
      support.point.x = snapshot.supports[offset + 3];
      support.point.y = snapshot.supports[offset + 4];
      support.normal.x = snapshot.supports[offset + 5];
      support.normal.y = snapshot.supports[offset + 6];

      const limb = internal.limbRefs[index];
      const config = LIMB_CONFIGS[index];
      limb.root.x = config.restRoot.x + rig.values[BODY_LEAN_INDEX];
      limb.root.y = config.restRoot.y - rig.values[BODY_LIFT_INDEX];
      offset = index * LIMB_STRIDE;
      limb.target.x = snapshot.limbs[offset];
      limb.target.y = snapshot.limbs[offset + 1];
      limb.joint.x = snapshot.limbs[offset + 2];
      limb.joint.y = snapshot.limbs[offset + 3];
      limb.end.x = snapshot.limbs[offset + 4];
      limb.end.y = snapshot.limbs[offset + 5];
      limb.upperAngle = snapshot.limbs[offset + 6];
      limb.lowerAngle = snapshot.limbs[offset + 7];
      limb.contactError = snapshot.limbs[offset + 8];
    }
    let clawOffset = 0;
    for(let clawIndex = 0; clawIndex < internal.clawList.length; clawIndex += 1){
      const claw = internal.clawList[clawIndex];
      claw.mode = snapshot.claws[clawOffset] === 1 ? "loaded" : "released";
      claw.cableT = snapshot.claws[clawOffset + 1];
      claw.point.x = snapshot.claws[clawOffset + 2];
      claw.point.y = snapshot.claws[clawOffset + 3];
      claw.contactError = snapshot.claws[clawOffset + 4];
      clawOffset += CLAW_STRIDE;
    }
    rig.transferGoal = snapshot.transferGoal;
    rig.lastValidTransferGoal = snapshot.transferGoal;
    rig.lastDt = snapshot.lastDt;
    rig.lastValidDt = snapshot.lastDt;
    rig.diagnostics.steps = snapshot.steps;
    rig.diagnostics.maxCableStretch = snapshot.maxCableStretch;
    rig.diagnostics.maxLoadedContactError = snapshot.maxLoadedContactError;
    rig.diagnostics.cableEnergy = snapshot.cableEnergy;
    rig.lastValidSteps = snapshot.steps;
    rig.lastValidMaxCableStretch = snapshot.maxCableStretch;
    rig.lastValidMaxLoadedContactError = snapshot.maxLoadedContactError;
    rig.lastValidCableEnergy = snapshot.cableEnergy;
    internal.recoveryCount = recoveries + 1;
    rig.diagnostics.recoveries = internal.recoveryCount;
    return true;
  }

  function createRig(seed){
    const values = new Float64Array(CHANNELS.length);
    const targets = new Float64Array(CHANNELS.length);
    for(let index = 0; index < CHANNELS.length; index += 1){
      const initial = LIMITS[CHANNELS[index]].default;
      values[index] = initial;
      targets[index] = initial;
    }
    const rig = {
      seed:Core.hashSeed(seed),
      values,
      targets,
      velocities:new Float64Array(CHANNELS.length),
      lastValidValues:new Float64Array(CHANNELS.length),
      lastValidTargets:new Float64Array(CHANNELS.length),
      lastValidVelocities:new Float64Array(CHANNELS.length),
      cable:new Float64Array(CABLE_NODES * 2),
      cablePrevious:new Float64Array(CABLE_NODES * 2),
      cableRestLengths:new Float64Array(CABLE_SEGMENTS),
      lastValidCable:new Float64Array(CABLE_NODES * 2),
      lastValidCablePrevious:new Float64Array(CABLE_NODES * 2),
      lastValidSupports:new Float64Array(LIMB_NAMES.length * SUPPORT_STRIDE),
      lastValidLimbs:new Float64Array(LIMB_NAMES.length * LIMB_STRIDE),
      lastValidClaws:new Float64Array(12 * CLAW_STRIDE),
      supports:createSupports(),
      limbs:createLimbs(),
      claws:createClaws(),
      cableIterations:CABLE_ITERATIONS,
      transferGoal:-1,
      lastValidTransferGoal:-1,
      lastDt:1 / 60,
      lastValidDt:1 / 60,
      lastValidSteps:0,
      lastValidMaxCableStretch:0,
      lastValidMaxLoadedContactError:0,
      lastValidCableEnergy:0,
      diagnostics:{recoveries:0, steps:0, maxCableStretch:0,
        maxLoadedContactError:0, cableEnergy:0},
    };
    createInternalReferences(rig);
    initialiseCable(rig);
    solveAllLimbs(rig);
    updateClaws(rig);
    computeMetrics(rig, rig.lastDt);
    hardenRig(rig);
    if(!internallyValid(rig, false)){
      throw new Error(`PerezOS rig initial pose is invalid: ${validatePose(rig).join("; ")}`);
    }
    saveLastValid(rig);
    return rig;
  }

  function setChannelTarget(rig, name, value){
    const index = channelIndex(name);
    if(!rig || !(rig.targets instanceof Float64Array) || index < 0 || typeof value !== "number"){
      return false;
    }
    rig.targets[index] = value;
    return true;
  }

  function requestGrip(rig, limbName, mode, cableT){
    const limbIndex = LIMB_NAMES.indexOf(limbName);
    if(!rig || limbIndex < 0 || (mode !== "loaded" && mode !== "release") ||
       !Number.isFinite(cableT)) return false;
    const internal = RIG_INTERNALS.get(rig);
    if(!internal) return false;
    if(!internallyValid(rig, false)){
      restoreLastValid(rig);
      return false;
    }
    const support = internal.supportRefs[limbIndex];
    if(mode === "release"){
      if(support.mode !== "loaded") return false;
      let safeSupport = null;
      for(let index = 0; index < LIMB_NAMES.length; index += 1){
        if(index === limbIndex) continue;
        const other = internal.supportRefs[index];
        if(other.mode === "loaded" && other.load >= SAFE_SINGLE_SUPPORT){
          safeSupport = other;
          break;
        }
      }
      if(!safeSupport) return false;
      const releasedLoad = support.load;
      support.mode = "released";
      support.load = 0;
      safeSupport.load += releasedLoad;
      if(rig.transferGoal === limbIndex) rig.transferGoal = -1;
      solveLimb(rig, internal.limbRefs[limbIndex], support, LIMB_CONFIGS[limbIndex]);
      updateClaws(rig);
      computeMetrics(rig, rig.lastDt);
      if(!internallyValid(rig, false)){
        restoreLastValid(rig);
        return false;
      }
      saveLastValid(rig);
      return true;
    }

    const boundedT = Core.clamp(cableT, 0, 1);
    cablePoint(rig, boundedT, internal.gripPoint);
    const targetX = internal.gripPoint.x;
    const targetY = internal.gripPoint.y;
    const limb = internal.limbRefs[limbIndex];
    const config = LIMB_CONFIGS[limbIndex];
    const distance = Math.hypot(targetX - limb.root.x, targetY - limb.root.y);
    if(distance >= config.upperLength + config.lowerLength - 0.001 ||
       distance <= Math.abs(config.upperLength - config.lowerLength) + 0.001) return false;

    support.mode = "loaded";
    support.cableT = boundedT;
    support.point.x = targetX;
    support.point.y = targetY;
    rig.transferGoal = limbIndex;
    solveLimb(rig, limb, support, config);
    updateClaws(rig);
    computeMetrics(rig, rig.lastDt);
    if(!internallyValid(rig, false)){
      restoreLastValid(rig);
      return false;
    }
    saveLastValid(rig);
    return true;
  }

  function solveRig(rig, dt){
    const internal = rig && RIG_INTERNALS.get(rig);
    if(!rig || !Number.isFinite(dt) || dt < 0){
      if(internal) restoreLastValid(rig);
      return false;
    }
    if(!internal) return false;
    if(!internallyValid(rig, true)){
      restoreLastValid(rig);
      return false;
    }
    computeMetrics(rig, rig.lastDt);
    if(!internallyValid(rig, true)){
      restoreLastValid(rig);
      return false;
    }
    if(dt === 0){
      for(let index = 0; index < CHANNELS.length; index += 1){
        const limit = LIMITS[CHANNELS[index]];
        rig.targets[index] = Core.clamp(rig.targets[index], limit.min, limit.max);
      }
      if(!internallyValid(rig, false)){
        restoreLastValid(rig);
        return false;
      }
      saveLastValid(rig);
      return true;
    }

    const boundedDt = Math.min(dt, MAX_DT);
    updateChannels(rig, boundedDt);
    updateSupportTransfer(rig, boundedDt);
    integrateCable(rig, boundedDt);
    updateSupportPoints(rig);
    solveAllLimbs(rig);
    updateClaws(rig);
    computeMetrics(rig, boundedDt);
    if(!internallyValid(rig, false)){
      restoreLastValid(rig);
      return false;
    }
    rig.lastDt = boundedDt;
    rig.diagnostics.steps += 1;
    saveLastValid(rig);
    return true;
  }

  function validatePose(rig){
    const errors = [];
    if(!rig || typeof rig !== "object"){
      return ["rig: invalid channel buffer"];
    }
    const internal = RIG_INTERNALS.get(rig);
    if(!internal) return ["rig: missing private configuration"];
    if(!runtimeTopologyValid(rig, internal)) errors.push("rig: invalid exact anatomy topology");
    if(errors.length > 0) return errors;
    if(!authoritativeBuffersUsable(rig, internal)) return ["rig: invalid physical state buffer"];
    if(!transitionWritable(rig, internal)) errors.push("rig: invalid writable state descriptors");
    const snapshot = internal.snapshot;
    if(rig.lastValidTransferGoal !== snapshot.transferGoal ||
       rig.lastValidDt !== snapshot.lastDt || rig.lastValidSteps !== snapshot.steps ||
       rig.lastValidMaxCableStretch !== snapshot.maxCableStretch ||
       rig.lastValidMaxLoadedContactError !== snapshot.maxLoadedContactError ||
       rig.lastValidCableEnergy !== snapshot.cableEnergy ||
       !buffersMatch(rig.lastValidValues, snapshot.values) ||
       !buffersMatch(rig.lastValidTargets, snapshot.targets) ||
       !buffersMatch(rig.lastValidVelocities, snapshot.velocities) ||
       !buffersMatch(rig.lastValidCable, snapshot.cable) ||
       !buffersMatch(rig.lastValidCablePrevious, snapshot.cablePrevious) ||
       !buffersMatch(rig.lastValidSupports, snapshot.supports) ||
       !buffersMatch(rig.lastValidLimbs, snapshot.limbs) ||
       !buffersMatch(rig.lastValidClaws, snapshot.claws)){
      errors.push("rig: invalid last-valid state");
    }
    if(!Number.isInteger(rig.transferGoal) || rig.transferGoal < -1 ||
       rig.transferGoal >= LIMB_NAMES.length) errors.push("rig: invalid transfer goal");
    else if(rig.transferGoal >= 0 && internal.supportRefs[rig.transferGoal].mode !== "loaded"){
      errors.push("rig: transfer goal must be loaded");
    }
    if(rig.cableIterations !== CABLE_ITERATIONS) errors.push("rig: invalid cable iterations");
    if(!Number.isFinite(rig.lastDt) || rig.lastDt < 0 || rig.lastDt > MAX_DT){
      errors.push("rig: invalid last dt");
    }
    let supportLoad = 0;
    let loadedSupports = 0;
    for(let index = 0; index < CHANNELS.length; index += 1){
      const name = CHANNELS[index];
      const limit = LIMITS[name];
      if(!Number.isFinite(rig.values[index])) errors.push(`${name}: non-finite value`);
      else if(rig.values[index] < limit.min - 1e-9 || rig.values[index] > limit.max + 1e-9){
        errors.push(`${name}: value outside limits`);
      }
      if(!Number.isFinite(rig.targets[index])) errors.push(`${name}: non-finite target`);
      else if(rig.targets[index] < limit.min - 1e-9 || rig.targets[index] > limit.max + 1e-9){
        errors.push(`${name}: target outside limits`);
      }
      if(!Number.isFinite(rig.velocities[index])) errors.push(`${name}: non-finite velocity`);
    }
    for(let index = 0; index < rig.cable.length; index += 1){
      if(!Number.isFinite(rig.cable[index])) errors.push(`cable: non-finite node ${index}`);
      if(!Number.isFinite(rig.cablePrevious[index])) errors.push(`cable: non-finite previous node ${index}`);
    }
    for(let segment = 0; segment < CABLE_SEGMENTS; segment += 1){
      const offset = segment * 2;
      const distance = Math.hypot(rig.cable[offset + 2] - rig.cable[offset],
        rig.cable[offset + 3] - rig.cable[offset + 1]);
      if(rig.cableRestLengths[segment] !== CABLE_REST_LENGTHS[segment]){
        errors.push(`cable: mutated rest geometry ${segment}`);
      }
      const stretch = Math.abs(distance - CABLE_REST_LENGTHS[segment]) /
        CABLE_REST_LENGTHS[segment];
      if(!Number.isFinite(stretch)) errors.push(`cable: non-finite stretch ${segment}`);
      else if(stretch >= MAX_CABLE_STRETCH) errors.push(`cable: segment ${segment} exceeds 3% stretch`);
    }
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const name = LIMB_NAMES[index];
      const support = internal.supportRefs[index];
      const limb = internal.limbRefs[index];
      const config = LIMB_CONFIGS[index];
      const claws = internal.clawGroups[index];
      if(rig.supports[name] !== support || rig.limbs[name] !== limb ||
         support.limb !== name || !limbStructureMatches(limb, config)){
        errors.push(`${name}: invalid authored topology`);
      }
      if(!support || (support.mode !== "loaded" && support.mode !== "released")){
        errors.push(`${name}: invalid support mode`);
        continue;
      }
      if(!support.point || !support.normal){
        errors.push(`${name}: invalid support geometry container`);
        continue;
      }
      if(!Number.isFinite(support.point.x) || !Number.isFinite(support.point.y)){
        errors.push(`${name}: non-finite support point`);
      }
      if(!Number.isFinite(support.cableT) || support.cableT < 0 || support.cableT > 1){
        errors.push(`${name}: invalid cable position`);
      }
      if(!Number.isFinite(support.load) || support.load < -1e-9 || support.load > 1 + 1e-9){
        errors.push(`${name}: invalid support load`);
      }else supportLoad += support.load;
      if(support.mode === "released" && support.load !== 0){
        errors.push(`${name}: released support must have zero load`);
      }
      if(support.mode === "loaded") loadedSupports += 1;
      if(support.mode === "loaded"){
        cablePoint(rig, support.cableT, internal.gripPoint);
        if(Math.hypot(support.point.x - internal.gripPoint.x,
            support.point.y - internal.gripPoint.y) > 1e-9){
          errors.push(`${name}: invalid cable contact point`);
        }
      }
      if(!limb || !limb.root || !limb.target || !limb.joint || !limb.end){
        errors.push(`${name}: invalid IK geometry container`);
        continue;
      }
      if(!Number.isFinite(limb.root.x) || !Number.isFinite(limb.root.y) ||
         !Number.isFinite(limb.target.x) || !Number.isFinite(limb.target.y) ||
         !Number.isFinite(limb.joint.x) || !Number.isFinite(limb.joint.y) ||
         !Number.isFinite(limb.end.x) || !Number.isFinite(limb.end.y) ||
         !Number.isFinite(limb.upperAngle) || !Number.isFinite(limb.lowerAngle) ||
         !Number.isFinite(limb.contactError)){
        errors.push(`${name}: non-finite IK pose`);
      }else{
        if(Math.abs(limb.root.x - (config.restRoot.x + rig.values[BODY_LEAN_INDEX])) > 1e-9 ||
           Math.abs(limb.root.y - (config.restRoot.y - rig.values[BODY_LIFT_INDEX])) > 1e-9){
          errors.push(`${name}: invalid IK root`);
        }
        if(!limbTargetMatches(rig, limb, support, config)){
          errors.push(`${name}: invalid IK target`);
        }
        if(!limbAnglesMatch(limb)) errors.push(`${name}: invalid IK angles`);
        if(!limbBranchMatches(limb, config)) errors.push(`${name}: invalid IK branch`);
        if(Math.abs(Math.hypot(limb.joint.x - limb.root.x, limb.joint.y - limb.root.y) -
          config.upperLength) > 1e-5 ||
          Math.abs(Math.hypot(limb.end.x - limb.joint.x, limb.end.y - limb.joint.y) -
          config.lowerLength) > 1e-5) errors.push(`${name}: invalid bone length`);
        const overlap = bodyOverlapKind(rig, limb, support, config, claws);
        if(overlap === 1) errors.push(`${name}: endpoint enters body overlap zone`);
        else if(overlap === 2) errors.push(`${name}: segment enters body overlap zone`);
        else if(overlap === 3) errors.push(`${name}: contact enters body overlap zone`);
      }
      if(!Number.isFinite(support.normal.x) || !Number.isFinite(support.normal.y) ||
         Math.abs(Math.hypot(support.normal.x, support.normal.y) - 1) > 1e-6){
        errors.push(`${name}: support requires a unit normal`);
      }
      const limbError = support.mode === "loaded" ?
        Math.hypot(limb.end.x - support.point.x, limb.end.y - support.point.y) : 0;
      if(!Number.isFinite(limb.contactError) || limbError >= MAX_CONTACT_ERROR){
        errors.push(`${name}: loaded contact error exceeds one pixel`);
      }else if(Math.abs(limb.contactError - limbError) > 1e-9){
        errors.push(`${name}: invalid stored contact error`);
      }
      for(let clawIndex = 0; clawIndex < claws.length; clawIndex += 1){
        const clawId = CLAW_IDS_BY_LIMB[index][clawIndex];
        const claw = claws[clawIndex];
        if(rig.claws[clawId] !== claw || !claw || !claw.point ||
           claw.id !== clawId || claw.limb !== name ||
           claw.index !== clawIndex + 1 || claw.mode !== support.mode ||
           !Number.isFinite(claw.cableT) || Math.abs(claw.cableT - support.cableT) > 1e-12 ||
           !Number.isFinite(claw.point.x) || !Number.isFinite(claw.point.y) ||
           !Number.isFinite(claw.contactError)){
          errors.push(`${clawId}: invalid grip state`);
        }else{
          if(Math.hypot(claw.point.x - limb.end.x, claw.point.y - limb.end.y) > 1e-9){
            errors.push(`${clawId}: invalid limb contact point`);
          }
          const clawError = support.mode === "loaded" ?
            Math.hypot(claw.point.x - support.point.x, claw.point.y - support.point.y) : 0;
          if(clawError >= MAX_CONTACT_ERROR){
            errors.push(`${clawId}: loaded contact error exceeds one pixel`);
          }else if(Math.abs(claw.contactError - clawError) > 1e-9){
            errors.push(`${clawId}: invalid stored contact error`);
          }
        }
      }
    }
    if(loadedSupports < 1) errors.push("supports: at least one loaded support is required");
    if(Math.abs(supportLoad - 1) >= 1e-5) errors.push("supports: loads must sum to one");
    if(!Number.isInteger(rig.diagnostics.recoveries) || rig.diagnostics.recoveries < 0 ||
       rig.diagnostics.recoveries !== internal.recoveryCount){
      errors.push("diagnostics: invalid recovery count");
    }
    if(!Number.isInteger(rig.diagnostics.steps) || rig.diagnostics.steps < 0){
      errors.push("diagnostics: invalid step count");
    }
    if(!Number.isFinite(rig.diagnostics.maxCableStretch) ||
       rig.diagnostics.maxCableStretch < 0 ||
       rig.diagnostics.maxCableStretch >= MAX_CABLE_STRETCH){
      errors.push("cable: invalid stretch metric");
    }
    if(!Number.isFinite(rig.diagnostics.maxLoadedContactError) ||
       rig.diagnostics.maxLoadedContactError < 0 ||
       rig.diagnostics.maxLoadedContactError >= MAX_CONTACT_ERROR){
      errors.push("contacts: invalid loaded contact metric");
    }
    if(!Number.isFinite(rig.diagnostics.cableEnergy) || rig.diagnostics.cableEnergy < 0){
      errors.push("cable: non-finite energy");
    }
    return errors;
  }

  function poseHash(rig){
    let hash = 0x811c9dc5;
    const bytes = new Uint8Array(8);
    const view = new DataView(bytes.buffer);
    function addNumber(value){
      view.setFloat64(0, value, true);
      for(let index = 0; index < bytes.length; index += 1){
        hash ^= bytes[index];
        hash = Math.imul(hash, 0x01000193);
      }
    }
    function addString(value){
      const text = String(value);
      for(let index = 0; index < text.length; index += 1){
        const code = text.charCodeAt(index);
        hash ^= code & 0xff;
        hash = Math.imul(hash, 0x01000193);
        hash ^= code >>> 8;
        hash = Math.imul(hash, 0x01000193);
      }
    }
    function addBuffer(buffer){
      for(let index = 0; index < buffer.length; index += 1) addNumber(buffer[index]);
    }
    const internal = RIG_INTERNALS.get(rig);
    addNumber(rig.seed);
    addNumber(runtimeTopologyValid(rig, internal) ? 1 : 0);
    addNumber(transitionWritable(rig, internal) ? 1 : 0);
    addNumber(rig.diagnostics === internal.diagnostics ? 1 : 0);
    addNumber(rig.supports === internal.supportsContainer ? 1 : 0);
    addNumber(rig.limbs === internal.limbsContainer ? 1 : 0);
    addNumber(rig.claws === internal.clawsContainer ? 1 : 0);
    addNumber(rig.clawGroups === internal.clawGroups ? 1 : 0);
    addNumber(rig.clawList === internal.clawList ? 1 : 0);
    addNumber(rig.values === internal.buffers.values ? 1 : 0);
    addNumber(rig.targets === internal.buffers.targets ? 1 : 0);
    addNumber(rig.velocities === internal.buffers.velocities ? 1 : 0);
    addNumber(rig.lastValidValues === internal.buffers.lastValidValues ? 1 : 0);
    addNumber(rig.lastValidTargets === internal.buffers.lastValidTargets ? 1 : 0);
    addNumber(rig.lastValidVelocities === internal.buffers.lastValidVelocities ? 1 : 0);
    addNumber(rig.cable === internal.buffers.cable ? 1 : 0);
    addNumber(rig.cablePrevious === internal.buffers.cablePrevious ? 1 : 0);
    addNumber(rig.cableRestLengths === internal.buffers.cableRestLengths ? 1 : 0);
    addNumber(rig.lastValidCable === internal.buffers.lastValidCable ? 1 : 0);
    addNumber(rig.lastValidCablePrevious === internal.buffers.lastValidCablePrevious ? 1 : 0);
    addNumber(rig.lastValidSupports === internal.buffers.lastValidSupports ? 1 : 0);
    addNumber(rig.lastValidLimbs === internal.buffers.lastValidLimbs ? 1 : 0);
    addNumber(rig.lastValidClaws === internal.buffers.lastValidClaws ? 1 : 0);
    addBuffer(rig.values);
    addBuffer(rig.targets);
    addBuffer(rig.velocities);
    addBuffer(rig.lastValidValues);
    addBuffer(rig.lastValidTargets);
    addBuffer(rig.lastValidVelocities);
    for(let index = 0; index < rig.cable.length; index += 1){
      addNumber(rig.cable[index]);
      addNumber(rig.cablePrevious[index]);
      addNumber(rig.lastValidCable[index]);
      addNumber(rig.lastValidCablePrevious[index]);
    }
    addBuffer(rig.cableRestLengths);
    addBuffer(rig.lastValidSupports);
    addBuffer(rig.lastValidLimbs);
    addBuffer(rig.lastValidClaws);
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = internal.supportRefs[index];
      addNumber(rig.supports[LIMB_NAMES[index]] === support ? 1 : 0);
      addNumber(support.point === internal.supportPoints[index] ? 1 : 0);
      addNumber(support.normal === internal.supportNormals[index] ? 1 : 0);
      addString(support.limb);
      addString(support.mode);
      addNumber(support.mode === "loaded" ? 1 : 0);
      addNumber(support.cableT);
      addNumber(support.load);
      addNumber(support.point.x);
      addNumber(support.point.y);
      addNumber(support.normal.x);
      addNumber(support.normal.y);
      const limb = internal.limbRefs[index];
      addNumber(rig.limbs[LIMB_NAMES[index]] === limb ? 1 : 0);
      addNumber(limb.restRoot === internal.limbRestRoots[index] ? 1 : 0);
      addNumber(limb.restJoint === internal.limbRestJoints[index] ? 1 : 0);
      addNumber(limb.restEnd === internal.limbRestEnds[index] ? 1 : 0);
      addNumber(limb.root === internal.limbRoots[index] ? 1 : 0);
      addNumber(limb.target === internal.limbTargets[index] ? 1 : 0);
      addNumber(limb.joint === internal.limbJoints[index] ? 1 : 0);
      addNumber(limb.end === internal.limbEnds[index] ? 1 : 0);
      addString(limb.name);
      addString(limb.upperPart);
      addString(limb.lowerPart);
      addString(limb.palmPart);
      addNumber(limb.restRoot.x);
      addNumber(limb.restRoot.y);
      addNumber(limb.restJoint.x);
      addNumber(limb.restJoint.y);
      addNumber(limb.restEnd.x);
      addNumber(limb.restEnd.y);
      addNumber(limb.root.x);
      addNumber(limb.root.y);
      addNumber(limb.baseRootX);
      addNumber(limb.baseRootY);
      addNumber(limb.restX);
      addNumber(limb.restY);
      addNumber(limb.upperLength);
      addNumber(limb.lowerLength);
      addNumber(limb.bend);
      addNumber(limb.reachXIndex);
      addNumber(limb.reachYIndex);
      addNumber(limb.liftIndex);
      addNumber(limb.target.x);
      addNumber(limb.target.y);
      addNumber(limb.joint.x);
      addNumber(limb.joint.y);
      addNumber(limb.end.x);
      addNumber(limb.end.y);
      addNumber(limb.upperAngle);
      addNumber(limb.lowerAngle);
      addNumber(limb.contactError);
      const claws = internal.clawGroups[index];
      for(let clawIndex = 0; clawIndex < claws.length; clawIndex += 1){
        const claw = claws[clawIndex];
        const flatClawIndex = index * 3 + clawIndex;
        addNumber(rig.claws[CLAW_IDS[flatClawIndex]] === claw ? 1 : 0);
        addNumber(claw.point === internal.clawPoints[flatClawIndex] ? 1 : 0);
        addString(claw.id);
        addString(claw.limb);
        addString(claw.mode);
        addNumber(claw.index);
        addNumber(claw.mode === "loaded" ? 1 : 0);
        addNumber(claw.cableT);
        addNumber(claw.point.x);
        addNumber(claw.point.y);
        addNumber(claw.contactError);
      }
    }
    addNumber(rig.transferGoal);
    addNumber(rig.lastValidTransferGoal);
    addNumber(rig.lastDt);
    addNumber(rig.lastValidDt);
    addNumber(rig.diagnostics.steps);
    addNumber(rig.diagnostics.maxCableStretch);
    addNumber(rig.diagnostics.maxLoadedContactError);
    addNumber(rig.diagnostics.cableEnergy);
    addNumber(rig.lastValidSteps);
    addNumber(rig.lastValidMaxCableStretch);
    addNumber(rig.lastValidMaxLoadedContactError);
    addNumber(rig.lastValidCableEnergy);
    addNumber(rig.cableIterations);
    return (hash >>> 0).toString(16).padStart(8, "0");
  }

  NS.Rig = Object.freeze({CHANNELS, CHANNEL_GROUPS, LIMITS, createRig,
    setChannelTarget, requestGrip, solveRig, poseHash, validatePose, channelIndex});
})(typeof window !== "undefined" ? window : globalThis);
