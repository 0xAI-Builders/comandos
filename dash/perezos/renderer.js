(function(root){
  "use strict";

  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  if(!NS.Art) throw new Error("ComandOSPerezOS.Art must load before Renderer");
  if(!NS.Rig) throw new Error("ComandOSPerezOS.Rig must load before Renderer");

  const Art = NS.Art;
  const Rig = NS.Rig;
  const QUALITY = Object.freeze({
    FULL:"full", BALANCED:"balanced", ECONOMY:"economy", STATIC:"static",
  });
  const QUALITY_NAMES = Object.freeze([
    QUALITY.FULL, QUALITY.BALANCED, QUALITY.ECONOMY, QUALITY.STATIC,
  ]);
  const LINE_ROW_OFFSET = 32;
  const LINE_ROW_COUNT = Art.WORLD.height + LINE_ROW_OFFSET * 2;
  const LINE_MIN_SENTINEL = 32767;
  const LINE_MAX_SENTINEL = -32768;
  const OCCLUSION_GROUPS = Object.freeze([
    "rear-cable-depth", "rear-limbs", "axial-deep-fur", "torso",
    "front-limbs", "face", "claws-contact-masks", "medium-fine-fur",
    "props", "front-cable", "state-rim-light",
  ]);
  const LIMB_NAMES = Object.freeze([
    "front-left", "front-right", "rear-left", "rear-right",
  ]);
  const CLAW_IDS = Object.freeze([
    "claw-front-left-1", "claw-front-left-2", "claw-front-left-3",
    "claw-front-right-1", "claw-front-right-2", "claw-front-right-3",
    "claw-rear-left-1", "claw-rear-left-2", "claw-rear-left-3",
    "claw-rear-right-1", "claw-rear-right-2", "claw-rear-right-3",
  ]);
  const REAR_IDS = Object.freeze([
    "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr",
    "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr",
  ]);
  const TORSO_IDS = Object.freeze([
    "pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid", "neck-upper",
  ]);
  const FRONT_IDS = Object.freeze([
    "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl",
    "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl",
  ]);
  const FACE_IDS = Object.freeze([
    "skull", "face-mask", "muzzle", "jaw", "nose", "eye-left", "eye-right",
    "lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
  ]);
  const FUR_IDS = Object.freeze(["fur-belly", "fur-head"]);
  const RENDERERS = new WeakMap();
  const PART_BY_ID = Object.create(null);
  const VARIANT_KEYS = Object.create(null);
  const PROP_KEYS = Object.create(null);
  const POSE_RECORDS = Object.create(null);
  const REST_X = Object.create(null);
  const REST_Y = Object.create(null);
  const AXIAL_LEVEL = Object.create(null);
  const CLAW_CURL_INDEX = Object.create(null);
  const CLAW_SPREAD_INDEX = Object.create(null);
  const CLAW_FAN = Object.create(null);
  const PROP_CHANNEL = Object.freeze({
    corona:"prop-corona-tilt", casco:"prop-casco-tilt", visor:"prop-visor-open",
    fuego:"prop-fuego-height", bufanda:"prop-bufanda-sway", huevo:"prop-huevo-wobble",
  });
  const DETAIL_CELLS = Object.freeze([
    Object.freeze({part:"fur-head", x:76, y:0, width:72, height:60}),
    Object.freeze({part:"fur-back", x:60, y:40, width:100, height:112}),
    Object.freeze({part:"fur-belly", x:77, y:74, width:66, height:72}),
  ]);
  const AUTHORED_DITHER = Object.freeze([
    Object.freeze({part:"fur-head", points:Object.freeze([
      90,14, 97,10, 105,16, 119,12, 128,17, 136,22,
    ])}),
    Object.freeze({part:"fur-back", points:Object.freeze([
      76,59, 83,71, 91,64, 139,58, 146,73, 132,83,
    ])}),
    Object.freeze({part:"fur-belly", points:Object.freeze([
      79,91, 87,105, 96,116, 126,102, 136,119, 145,94,
    ])}),
  ]);
  const DITHER_PIXEL_COUNT = 18;
  const HEX_COLOR_PATTERN = /^#[0-9a-f]{6}$/i;
  const ACCENT_DESCRIPTOR = Object.freeze({
    rim:"brand", deepShadowBias:"line", cableNodeFill:"panel",
    cableNodeRing:"brand", cableNodeEdge:"line", prop:"brand", propEdge:"line",
  });

  for(let index = 0; index < Art.PARTS.length; index += 1){
    const part = Art.PARTS[index];
    PART_BY_ID[part.id] = part;
    REST_X[part.id] = part.bounds[0] + part.pivot[0];
    REST_Y[part.id] = part.bounds[1] + part.pivot[1];
    const keys = Object.create(null);
    keys.base = part.id;
    if(part.states.loaded) keys.loaded = `${part.id}@loaded`;
    if(part.states.searching) keys.searching = `${part.id}@searching`;
    if(part.states.turned) keys.turned = `${part.id}@turned`;
    VARIANT_KEYS[part.id] = Object.freeze(keys);
    POSE_RECORDS[part.id] = Object.seal({kind:0, limb:"", fraction:0,
      offsetX:0, offsetY:0});
  }
  for(const name in Art.PROPS) PROP_KEYS[name] = `prop:${name}`;

  function restAnchor(id){
    return {x:REST_X[id], y:REST_Y[id]};
  }

  function configureLimb(limbName, upperId, lowerId, middleId, palmId){
    const lower = restAnchor(lowerId);
    const middle = restAnchor(middleId);
    const palm = restAnchor(palmId);
    const span = Math.hypot(palm.x - lower.x, palm.y - lower.y);
    const middleSpan = Math.hypot(middle.x - lower.x, middle.y - lower.y);
    POSE_RECORDS[upperId] = Object.freeze({kind:1, limb:limbName, fraction:0,
      offsetX:0, offsetY:0});
    POSE_RECORDS[lowerId] = Object.freeze({kind:2, limb:limbName, fraction:0,
      offsetX:0, offsetY:0});
    POSE_RECORDS[middleId] = Object.freeze({kind:3, limb:limbName,
      fraction:span > 0 ? middleSpan / span : 0.7, offsetX:0, offsetY:0});
    POSE_RECORDS[palmId] = Object.freeze({kind:4, limb:limbName, fraction:1,
      offsetX:0, offsetY:0});
    for(let clawIndex = 1; clawIndex <= 3; clawIndex += 1){
      const clawId = `claw-${limbName}-${clawIndex}`;
      const clawAnchor = restAnchor(clawId);
      POSE_RECORDS[clawId] = Object.freeze({kind:5, limb:limbName, fraction:1,
        offsetX:clawAnchor.x - palm.x, offsetY:clawAnchor.y - palm.y});
    }
  }

  configureLimb("front-left", "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl");
  configureLimb("front-right", "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr");
  configureLimb("rear-left", "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl");
  configureLimb("rear-right", "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr");

  function authoredStaticPose(){
    const rig = Rig.createRig("renderer-authored-static-pose");
    const limbs = {};
    const supports = {};
    const claws = {};
    const point = value => Object.freeze({x:value.x, y:value.y});
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const name = LIMB_NAMES[index];
      const limb = rig.limbs[name];
      const support = rig.supports[name];
      limbs[name] = Object.freeze({root:point(limb.root),joint:point(limb.joint),
        end:point(limb.end)});
      supports[name] = Object.freeze({mode:support.mode,load:support.load,
        point:point(support.point)});
    }
    for(let index = 0; index < CLAW_IDS.length; index += 1){
      const id = CLAW_IDS[index];
      claws[id] = Object.freeze({mode:rig.claws[id].mode,point:point(rig.claws[id].point)});
    }
    return Object.freeze({cable:Object.freeze(Array.from(rig.cable)),
      limbs:Object.freeze(limbs),supports:Object.freeze(supports),
      claws:Object.freeze(claws)});
  }

  const STATIC_POSE = authoredStaticPose();

  for(let index = 0; index < CLAW_IDS.length; index += 1){
    const id = CLAW_IDS[index];
    CLAW_CURL_INDEX[id] = Rig.channelIndex(`${id}-curl`);
    CLAW_SPREAD_INDEX[id] = Rig.channelIndex(`${id}-spread`);
    CLAW_FAN[id] = index % 3 - 1;
  }

  AXIAL_LEVEL.pelvis = 0;
  AXIAL_LEVEL.abdomen = 1;
  AXIAL_LEVEL["fur-belly"] = 1;
  AXIAL_LEVEL.ribcage = 2;
  AXIAL_LEVEL["fur-back"] = 2;
  AXIAL_LEVEL["neck-lower"] = 3;
  AXIAL_LEVEL["neck-mid"] = 4;
  AXIAL_LEVEL["neck-upper"] = 5;
  for(let index = 0; index < FACE_IDS.length; index += 1){
    AXIAL_LEVEL[FACE_IDS[index]] = 6;
  }
  AXIAL_LEVEL["fur-head"] = 6;

  const INDEX = Object.freeze({
    bodyLean:Rig.channelIndex("body-lean-x"),
    bodyLift:Rig.channelIndex("body-lift"),
    pelvisX:Rig.channelIndex("spine-pelvis-x"),
    pelvisY:Rig.channelIndex("spine-pelvis-y"),
    pelvisAngle:Rig.channelIndex("spine-pelvis-angle"),
    lowerAngle:Rig.channelIndex("spine-lower-angle"),
    midAngle:Rig.channelIndex("spine-mid-angle"),
    upperAngle:Rig.channelIndex("spine-upper-angle"),
    neckLower:Rig.channelIndex("neck-lower-angle"),
    neckMid:Rig.channelIndex("neck-mid-angle"),
    neckUpper:Rig.channelIndex("neck-upper-angle"),
    headYaw:Rig.channelIndex("head-yaw"),
    headPitch:Rig.channelIndex("head-pitch"),
    headRoll:Rig.channelIndex("head-roll"),
    faceTurn:Rig.channelIndex("face-turn"),
    jawOpen:Rig.channelIndex("jaw-open"),
    muzzleLift:Rig.channelIndex("muzzle-lift"),
    noseTwitch:Rig.channelIndex("nose-twitch"),
    eyeLeftX:Rig.channelIndex("eye-left-look-x"),
    eyeLeftY:Rig.channelIndex("eye-left-look-y"),
    eyeRightX:Rig.channelIndex("eye-right-look-x"),
    eyeRightY:Rig.channelIndex("eye-right-look-y"),
    lidLeftUpper:Rig.channelIndex("lid-left-upper"),
    lidLeftLower:Rig.channelIndex("lid-left-lower"),
    lidRightUpper:Rig.channelIndex("lid-right-upper"),
    lidRightLower:Rig.channelIndex("lid-right-lower"),
    chestExpand:Rig.channelIndex("chest-expand"),
    bellyCompress:Rig.channelIndex("belly-compress"),
    browLeft:Rig.channelIndex("brow-left-lift"),
    browRight:Rig.channelIndex("brow-right-lift"),
    cheekLeft:Rig.channelIndex("cheek-left-puff"),
    cheekRight:Rig.channelIndex("cheek-right-puff"),
    furHeadCrest:Rig.channelIndex("fur-head-crest"),
    furHeadCheekLeft:Rig.channelIndex("fur-head-cheek-left"),
    furHeadCheekRight:Rig.channelIndex("fur-head-cheek-right"),
    furHeadNape:Rig.channelIndex("fur-head-nape"),
    furNeckLeft:Rig.channelIndex("fur-neck-ruff-left"),
    furNeckRight:Rig.channelIndex("fur-neck-ruff-right"),
    furBackShoulder:Rig.channelIndex("fur-back-shoulder"),
    furBackMid:Rig.channelIndex("fur-back-mid"),
    furBackRump:Rig.channelIndex("fur-back-rump"),
    furBellyChest:Rig.channelIndex("fur-belly-chest"),
    furBellyMid:Rig.channelIndex("fur-belly-mid"),
    furBellyFlank:Rig.channelIndex("fur-belly-flank"),
    lightKey:Rig.channelIndex("light-key-intensity"),
    lightFill:Rig.channelIndex("light-fill-intensity"),
    lightRim:Rig.channelIndex("light-rim-intensity"),
    lightLoaded:Rig.channelIndex("light-loaded-pulse"),
    lightSearching:Rig.channelIndex("light-searching-pulse"),
    lightVisor:Rig.channelIndex("light-visor-glow"),
    flShoulder:Rig.channelIndex("front-left-shoulder-angle"),
    flElbow:Rig.channelIndex("front-left-elbow-angle"),
    frShoulder:Rig.channelIndex("front-right-shoulder-angle"),
    frElbow:Rig.channelIndex("front-right-elbow-angle"),
    rlShoulder:Rig.channelIndex("rear-left-shoulder-angle"),
    rlElbow:Rig.channelIndex("rear-left-elbow-angle"),
    rrShoulder:Rig.channelIndex("rear-right-shoulder-angle"),
    rrElbow:Rig.channelIndex("rear-right-elbow-angle"),
  });

  const POSE_SNAPSHOT_LENGTH = Rig.CHANNELS.length + 18 + 4 * 9 + 4 * 7 + 12 * 5;

  function defaultCanvasFactory(width, height){
    if(!root.document || typeof root.document.createElement !== "function"){
      throw new Error("PerezOS Renderer requires canvasFactory outside a browser");
    }
    const canvas = root.document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    return canvas;
  }

  function themeName(theme){
    return theme === "dark" || theme === "light" ? theme : "";
  }

  function validHexColor(color){
    return typeof color === "string" && HEX_COLOR_PATTERN.test(color);
  }

  function createAccentPalette(){
    return Object.seal({brand:"", panel:"", line:""});
  }

  function accentChanged(internal, colors){
    return internal.accent.brand !== colors.brand ||
      internal.accent.panel !== colors.panel ||
      internal.accent.line !== colors.line;
  }

  function syncAccentPalette(internal, colors){
    const brand = colors.brand;
    const panel = colors.panel;
    const line = colors.line;
    if(internal.accent.brand === brand && internal.accent.panel === panel &&
       internal.accent.line === line) return false;
    internal.accent.brand = brand;
    internal.accent.panel = panel;
    internal.accent.line = line;
    internal.accentRevision += 1;
    return true;
  }

  function buildDetailPage(internal, theme, palette){
    const canvas = internal.canvasFactory(Art.WORLD.width, Art.WORLD.height);
    if(!canvas || typeof canvas.getContext !== "function"){
      throw new Error("PerezOS detail cache requires a canvas");
    }
    const ctx = canvas.getContext("2d", {alpha:true});
    if(!ctx) throw new Error("PerezOS detail cache requires a 2d canvas context");
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, Art.WORLD.width, Art.WORLD.height);
    ctx.fillStyle = palette[1];
    for(let y = 54; y <= 140; y += 9){
      ctx.fillRect(71 + ((y / 9) & 1) * 4, y, 3, 1);
      ctx.fillRect(148 - ((y / 9) & 1) * 3, y + 3, 2, 1);
    }
    ctx.fillStyle = palette[5];
    for(let y = 62; y <= 132; y += 8){
      ctx.fillRect(92 + ((y / 8) & 3) * 5, y, 4, 1);
      ctx.fillRect(101 + ((y / 8 + 1) & 3) * 6, y + 3, 2, 1);
    }
    ctx.fillStyle = palette[7];
    for(let x = 88; x <= 138; x += 6){
      ctx.fillRect(x, 10 + (x & 3), 2, 1);
      ctx.fillRect(91 + ((x / 6) & 3) * 9, 99 + (x & 7), 2, 1);
    }
    return canvas;
  }

  function buildThemePage(internal, theme){
    const atlas = Art.buildAtlas(internal.canvasFactory, theme);
    const detail = buildDetailPage(internal, theme, atlas.palette);
    const page = Object.freeze({atlas, detail});
    internal.pages[theme] = page;
    internal.atlasBuilds += 1;
    internal.atlasBytes += atlas.canvas.width * atlas.canvas.height * 4;
    internal.retainedCacheBytes += detail.width * detail.height * 4;
    return page;
  }

  function createRenderer(canvas, options){
    if(!canvas || typeof canvas.getContext !== "function"){
      throw new TypeError("createRenderer requires a canvas");
    }
    const ctx = canvas.getContext("2d", {alpha:true});
    if(!ctx) throw new Error("PerezOS Renderer requires a 2d canvas context");
    const factory = options && typeof options.canvasFactory === "function" ?
      options.canvasFactory : defaultCanvasFactory;
    const initialTheme = options && themeName(options.theme) ? options.theme : "dark";
    const poseSnapshot = new Float64Array(POSE_SNAPSHOT_LENGTH);
    const anchor = new Int32Array(2);
    const groupCounts = new Int16Array(OCCLUSION_GROUPS.length);
    const lineMinimums = new Int16Array(LINE_ROW_COUNT);
    const lineMaximums = new Int16Array(LINE_ROW_COUNT);
    const renderer = Object.seal({canvas});
    const internal = {
      canvas, ctx, canvasFactory:factory, pages:{dark:null, light:null},
      theme:initialTheme, page:null, poseSnapshot, anchor, groupCounts,
      lineMinimums, lineMaximums,
      poseReady:false, lastRig:null, destroyed:false, dirty:true,
      viewportWidth:0, viewportHeight:0, dpr:0, backingWidth:0, backingHeight:0,
      camera:null, cameraRevision:0, paletteRevision:0, accentRevision:0,
      accent:createAccentPalette(), manualRevision:0,
      poseRevision:0, contextRevision:0, contactRevision:0, propRevision:0,
      renders:0, skippedClean:0, atlasBuilds:0, atlasBytes:0,
      retainedCacheBytes:0, typedArrayBytes:poseSnapshot.byteLength +
        anchor.byteLength + groupCounts.byteLength + lineMinimums.byteLength +
        lineMaximums.byteLength,
      quality:"", detailPolicy:"", dynamicDitherPixels:0, contactMasks:0,
      contextReady:false, status:"", role:"", costume:"", pressure:"",
      contextTheme:"", expanded:false, scanOffset:0, scanChanged:false,
      scanInvalid:false, currentMaskBits:0, lastMaskBits:-1,
    };
    internal.page = buildThemePage(internal, initialTheme);
    RENDERERS.set(renderer, internal);
    return renderer;
  }

  function setViewport(renderer, width, height, dpr){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed || !Number.isFinite(width) ||
       !Number.isFinite(height) || !Number.isFinite(dpr) || width <= 0 ||
       height <= 0 || dpr <= 0) return false;
    const logicalWidth = Math.floor(width);
    const logicalHeight = Math.floor(height);
    const effectiveDpr = Math.max(1, Math.floor(dpr));
    const backingWidth = logicalWidth * effectiveDpr;
    const backingHeight = logicalHeight * effectiveDpr;
    if(backingWidth < 1 || backingHeight < 1 || backingWidth > 8192 ||
       backingHeight > 8192) return false;
    if(internal.viewportWidth === width && internal.viewportHeight === height &&
       internal.dpr === dpr && internal.backingWidth === backingWidth &&
       internal.backingHeight === backingHeight) return false;
    try{
      const oldWidth = internal.canvas.width;
      const oldHeight = internal.canvas.height;
      try{
        if(oldWidth !== backingWidth) internal.canvas.width = backingWidth;
        if(oldHeight !== backingHeight) internal.canvas.height = backingHeight;
      }catch(error){
        try{
          if(internal.canvas.width !== oldWidth) internal.canvas.width = oldWidth;
          if(internal.canvas.height !== oldHeight) internal.canvas.height = oldHeight;
        }catch(rollbackError){
          // A hostile host canvas may reject both the requested size and rollback.
        }
        return false;
      }
    }catch(error){
      return false;
    }
    internal.viewportWidth = width;
    internal.viewportHeight = height;
    internal.dpr = dpr;
    internal.backingWidth = backingWidth;
    internal.backingHeight = backingHeight;
    const logicalCamera = Art.compactCamera(logicalWidth, logicalHeight);
    let scale = logicalCamera.scale * effectiveDpr;
    let cameraWidth = logicalCamera.width;
    let cameraHeight = logicalCamera.height;
    let sourceX = logicalCamera.sourceX;
    let sourceY = logicalCamera.sourceY;
    if(cameraWidth * scale > backingWidth || cameraHeight * scale > backingHeight){
      const physicalCamera = Art.compactCamera(backingWidth, backingHeight);
      scale = physicalCamera.scale;
      cameraWidth = physicalCamera.width;
      cameraHeight = physicalCamera.height;
      sourceX = physicalCamera.sourceX;
      sourceY = physicalCamera.sourceY;
    }
    internal.camera = Object.freeze({
      x:Math.floor((backingWidth - cameraWidth * scale) / 2),
      y:Math.floor((backingHeight - cameraHeight * scale) / 2),
      sourceX, sourceY, scale, width:cameraWidth, height:cameraHeight,
    });
    internal.cameraRevision += 1;
    internal.dirty = true;
    return true;
  }

  function setTheme(renderer, theme){
    const internal = RENDERERS.get(renderer);
    const normalized = themeName(theme);
    if(!internal || internal.destroyed || !normalized || normalized === internal.theme){
      return false;
    }
    try{
      internal.page = internal.pages[normalized] || buildThemePage(internal, normalized);
    }catch(error){
      return false;
    }
    internal.theme = normalized;
    internal.paletteRevision += 1;
    internal.dirty = true;
    return true;
  }

  function validQuality(quality){
    for(let index = 0; index < QUALITY_NAMES.length; index += 1){
      if(quality === QUALITY_NAMES[index]) return true;
    }
    return false;
  }

  function validInputs(rig, context){
    return !!rig && rig.values instanceof Float64Array &&
      rig.values.length === Rig.CHANNELS.length && rig.cable instanceof Float64Array &&
      rig.cable.length === 18 && !!rig.limbs && !!rig.claws && !!rig.supports &&
      !!context && typeof context === "object" && typeof context.status === "string" &&
      typeof context.role === "string" && typeof context.costume === "string" &&
      typeof context.contextPressure === "string" && typeof context.theme === "string" &&
      typeof context.expanded === "boolean" && !!context.colors &&
      typeof context.colors === "object" && validHexColor(context.colors.brand) &&
      validHexColor(context.colors.panel) && validHexColor(context.colors.line);
  }

  function visualContextChanged(internal, context){
    return !internal.contextReady || internal.status !== context.status ||
      internal.role !== context.role || internal.costume !== context.costume ||
      internal.pressure !== context.contextPressure ||
      internal.contextTheme !== context.theme || internal.expanded !== context.expanded ||
      accentChanged(internal, context.colors);
  }

  function saveVisualContext(internal, context){
    const oldCostume = internal.costume;
    internal.status = context.status;
    internal.role = context.role;
    internal.costume = context.costume;
    internal.pressure = context.contextPressure;
    internal.contextTheme = context.theme;
    internal.expanded = context.expanded;
    internal.contextReady = true;
    internal.contextRevision += 1;
    if(oldCostume !== context.costume) internal.propRevision += 1;
  }

  function scanPoseValue(internal, value, write){
    if(!Number.isFinite(value)){
      internal.scanInvalid = true;
      return;
    }
    const offset = internal.scanOffset;
    if(internal.poseSnapshot[offset] !== value) internal.scanChanged = true;
    if(write) internal.poseSnapshot[offset] = value;
    internal.scanOffset = offset + 1;
  }

  function scanPose(internal, rig, write){
    internal.scanOffset = 0;
    internal.scanChanged = !internal.poseReady || internal.lastRig !== rig;
    internal.scanInvalid = false;
    for(let index = 0; index < rig.values.length; index += 1){
      scanPoseValue(internal, rig.values[index], write);
    }
    for(let index = 0; index < rig.cable.length; index += 1){
      scanPoseValue(internal, rig.cable[index], write);
    }
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const limb = rig.limbs[LIMB_NAMES[index]];
      if(!limb || !limb.root || !limb.joint || !limb.end) return -1;
      scanPoseValue(internal, limb.root.x, write);
      scanPoseValue(internal, limb.root.y, write);
      scanPoseValue(internal, limb.joint.x, write);
      scanPoseValue(internal, limb.joint.y, write);
      scanPoseValue(internal, limb.end.x, write);
      scanPoseValue(internal, limb.end.y, write);
      scanPoseValue(internal, limb.upperAngle, write);
      scanPoseValue(internal, limb.lowerAngle, write);
      scanPoseValue(internal, limb.contactError, write);
    }
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = rig.supports[LIMB_NAMES[index]];
      if(!support || !support.point || !support.normal ||
         (support.mode !== "loaded" && support.mode !== "released")) return -1;
      const mode = support.mode === "loaded" ? 1 : 0;
      scanPoseValue(internal, mode, write);
      scanPoseValue(internal, support.cableT, write);
      scanPoseValue(internal, support.load, write);
      scanPoseValue(internal, support.point.x, write);
      scanPoseValue(internal, support.point.y, write);
      scanPoseValue(internal, support.normal.x, write);
      scanPoseValue(internal, support.normal.y, write);
    }
    for(let index = 0; index < CLAW_IDS.length; index += 1){
      const claw = rig.claws[CLAW_IDS[index]];
      if(!claw || !claw.point || (claw.mode !== "loaded" && claw.mode !== "released")){
        return -1;
      }
      const mode = claw.mode === "loaded" ? 1 : 0;
      scanPoseValue(internal, mode, write);
      scanPoseValue(internal, claw.cableT, write);
      scanPoseValue(internal, claw.point.x, write);
      scanPoseValue(internal, claw.point.y, write);
      scanPoseValue(internal, claw.contactError, write);
    }
    if(internal.scanInvalid || internal.scanOffset !== POSE_SNAPSHOT_LENGTH) return -1;
    if(write){
      internal.poseReady = true;
      internal.lastRig = rig;
    }
    return internal.scanChanged ? 1 : 0;
  }

  function contactSnapshotChanged(internal, rig){
    if(!internal.poseReady || internal.lastRig !== rig) return true;
    let offset = Rig.CHANNELS.length + 18 + LIMB_NAMES.length * 9;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = rig.supports[LIMB_NAMES[index]];
      const mode = support.mode === "loaded" ? 1 : 0;
      if(internal.poseSnapshot[offset] !== mode ||
         internal.poseSnapshot[offset + 3] !== support.point.x ||
         internal.poseSnapshot[offset + 4] !== support.point.y) return true;
      offset += 7;
    }
    return false;
  }

  function screenX(internal, worldX){
    return internal.camera.x + (Math.round(worldX) - internal.camera.sourceX) *
      internal.camera.scale;
  }

  function screenY(internal, worldY){
    return internal.camera.y + (Math.round(worldY) - internal.camera.sourceY) *
      internal.camera.scale;
  }

  function globalAnchor(internal, part, rig, staticMode){
    let x = part.bounds[0] + part.pivot[0];
    let y = part.bounds[1] + part.pivot[1];
    if(staticMode){
      internal.anchor[0] = x;
      internal.anchor[1] = y;
      return;
    }
    const values = rig.values;
    const level = AXIAL_LEVEL[part.id];
    x += Math.round(values[INDEX.bodyLean]);
    y -= Math.round(values[INDEX.bodyLift]);
    if(level !== undefined){
      x += Math.round(values[INDEX.pelvisX]);
      y += Math.round(values[INDEX.pelvisY]);
    }
    if(level >= 1){
      x += Math.round(values[INDEX.pelvisAngle] * Math.min(level, 4) * 10);
      x += Math.round(values[INDEX.lowerAngle] * 3);
    }
    if(level >= 2){
      x += Math.round(values[INDEX.midAngle] * 4);
    }
    if(level >= 3){
      x += Math.round(values[INDEX.upperAngle] * 4);
    }
    if(level >= 4){
      x += Math.round(values[INDEX.neckLower] * 4);
    }
    if(level >= 5){
      x += Math.round(values[INDEX.neckMid] * 4);
    }
    if(level >= 6){
      x += Math.round(values[INDEX.neckUpper] * 4);
    }
    if(level >= 2){
      y -= Math.round(values[INDEX.chestExpand] * 2);
    }
    if(part.id === "abdomen" || part.id === "pelvis" || part.id === "fur-belly"){
      y += Math.round(values[INDEX.bellyCompress] * 2);
    }
    if(part.id === "fur-back"){
      x += Math.round((values[INDEX.furBackShoulder] + values[INDEX.furBackMid] * 0.7 +
        values[INDEX.furBackRump] * 0.45) * 2);
      y += Math.round((values[INDEX.furBackMid] + values[INDEX.furBackRump]) * 0.7);
    }else if(part.id === "fur-belly"){
      x += Math.round((values[INDEX.furBellyChest] - values[INDEX.furBellyFlank]) * 1.5);
      y += Math.round((values[INDEX.furBellyMid] + values[INDEX.furBellyFlank]) * 1.2);
    }else if(part.id === "fur-head"){
      x += Math.round((values[INDEX.furHeadCheekRight] -
        values[INDEX.furHeadCheekLeft]) * 1.5);
      y -= Math.round((values[INDEX.furHeadCrest] + values[INDEX.furHeadNape] * 0.5) * 2);
    }else if(part.id === "neck-lower" || part.id === "neck-mid" ||
             part.id === "neck-upper"){
      x += Math.round((values[INDEX.furNeckRight] - values[INDEX.furNeckLeft]) * 1.5);
    }
    const face = part.id === "skull" || part.id === "face-mask" ||
      part.id === "muzzle" || part.id === "jaw" || part.id === "nose" ||
      part.id === "eye-left" || part.id === "eye-right" ||
      part.id.startsWith("lid-") || part.id === "fur-head";
    if(face){
      x += Math.round((values[INDEX.headYaw] + values[INDEX.faceTurn]) * 3);
      y += Math.round(values[INDEX.headPitch] * 2);
    }
    if(part.id === "jaw") y += Math.round(values[INDEX.jawOpen] * 4);
    if(part.id === "muzzle") y -= Math.round(values[INDEX.muzzleLift] * 2);
    if(part.id === "nose") x += Math.round(values[INDEX.noseTwitch] * 2);
    if(part.id === "eye-left" || part.id.startsWith("lid-left")){
      x += Math.round(values[INDEX.eyeLeftX] * 2);
      y += Math.round(values[INDEX.eyeLeftY] * 2);
    }
    if(part.id === "eye-right" || part.id.startsWith("lid-right")){
      x += Math.round(values[INDEX.eyeRightX] * 2);
      y += Math.round(values[INDEX.eyeRightY] * 2);
    }
    if(part.id === "lid-left-upper") y += Math.round(values[INDEX.lidLeftUpper] * 2);
    if(part.id === "lid-left-lower") y -= Math.round(values[INDEX.lidLeftLower] * 2);
    if(part.id === "lid-right-upper") y += Math.round(values[INDEX.lidRightUpper] * 2);
    if(part.id === "lid-right-lower") y -= Math.round(values[INDEX.lidRightLower] * 2);
    if(part.id === "lid-left-upper") y -= Math.round(values[INDEX.browLeft] * 2);
    if(part.id === "lid-right-upper") y -= Math.round(values[INDEX.browRight] * 2);
    if(part.id === "muzzle" || part.id === "jaw"){
      x += Math.round((values[INDEX.cheekRight] - values[INDEX.cheekLeft]) * 1.5);
      y -= Math.round((values[INDEX.cheekRight] + values[INDEX.cheekLeft]) * 0.5);
    }
    internal.anchor[0] = Math.round(x);
    internal.anchor[1] = Math.round(y);
  }

  function anchorForPart(internal, part, rig, staticMode){
    const pose = POSE_RECORDS[part.id];
    if(!pose || pose.kind === 0){
      globalAnchor(internal, part, rig, staticMode);
      return;
    }
    const poseSource = staticMode ? STATIC_POSE : rig;
    const limb = poseSource.limbs[pose.limb];
    if(pose.kind === 1){
      internal.anchor[0] = Math.round(limb.root.x);
      internal.anchor[1] = Math.round(limb.root.y);
    }else if(pose.kind === 2){
      internal.anchor[0] = Math.round(limb.joint.x);
      internal.anchor[1] = Math.round(limb.joint.y);
    }else if(pose.kind === 3){
      internal.anchor[0] = Math.round(limb.joint.x +
        (limb.end.x - limb.joint.x) * pose.fraction);
      internal.anchor[1] = Math.round(limb.joint.y +
        (limb.end.y - limb.joint.y) * pose.fraction);
    }else if(pose.kind === 4){
      internal.anchor[0] = Math.round(limb.end.x);
      internal.anchor[1] = Math.round(limb.end.y);
    }else{
      const claw = poseSource.claws[part.id];
      internal.anchor[0] = Math.round(claw.point.x + pose.offsetX);
      internal.anchor[1] = Math.round(claw.point.y + pose.offsetY);
      if(!staticMode){
        internal.anchor[0] += Math.round(rig.values[CLAW_SPREAD_INDEX[part.id]] *
          CLAW_FAN[part.id] * 3);
        internal.anchor[1] += Math.round(rig.values[CLAW_CURL_INDEX[part.id]] * 3);
      }
    }
  }

  function supportLoadForPart(part, rig){
    const id = part.id;
    let limb = "";
    if(id.includes("fl") || id.startsWith("claw-front-left")) limb = "front-left";
    else if(id.includes("fr") || id.startsWith("claw-front-right")) limb = "front-right";
    else if(id.includes("rl") || id.startsWith("claw-rear-left")) limb = "rear-left";
    else if(id.includes("rr") || id.startsWith("claw-rear-right")) limb = "rear-right";
    if(limb) return rig.supports[limb].load;
    let load = 0;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const candidate = rig.supports[LIMB_NAMES[index]].load;
      if(candidate > load) load = candidate;
    }
    return load;
  }

  function turnBand(part, rig){
    const id = part.id;
    const values = rig.values;
    if(id === "pelvis") return Math.abs(values[INDEX.pelvisAngle]);
    if(id === "skull" || id === "face-mask" || id === "muzzle" ||
       id.startsWith("lid-")) return Math.max(Math.abs(values[INDEX.headYaw]),
         Math.abs(values[INDEX.faceTurn]));
    let limb = null;
    if(id.includes("fr")) limb = rig.limbs["front-right"];
    else if(id.includes("rr")) limb = rig.limbs["rear-right"];
    else if(id.includes("fl")) limb = rig.limbs["front-left"];
    else if(id.includes("rl")) limb = rig.limbs["rear-left"];
    if(limb){
      const restUpper = Math.atan2(limb.restJoint.y - limb.restRoot.y,
        limb.restJoint.x - limb.restRoot.x);
      const restLower = restUpper - Math.atan2(limb.restEnd.y - limb.restJoint.y,
        limb.restEnd.x - limb.restJoint.x);
      return id.includes("fore") || id.includes("lower") || id.includes("wrist") ||
        id.includes("ankle") || id.includes("palm") ?
        Math.abs(limb.lowerAngle - restLower) : Math.abs(limb.upperAngle - restUpper);
    }
    return 0;
  }

  function spriteKey(part, rig, context, staticMode){
    const keys = VARIANT_KEYS[part.id];
    const searching = context.status === "waiting" || context.contextPressure === "high" ||
      context.expanded === true;
    if(keys.searching && searching) return keys.searching;
    if(!staticMode && keys.turned && turnBand(part, rig) >= 0.45) return keys.turned;
    const loadedClaw = part.id.startsWith("claw-") &&
      (staticMode ? STATIC_POSE.claws[part.id].mode === "loaded" :
        rig.claws[part.id].mode === "loaded");
    if(keys.loaded && (loadedClaw || context.status === "working" ||
       (!staticMode && supportLoadForPart(part, rig) >= 0.66))) return keys.loaded;
    return keys.base;
  }

  function drawPart(internal, part, rig, context, staticMode){
    const key = spriteKey(part, rig, context, staticMode);
    const rect = internal.page.atlas.rects[key];
    anchorForPart(internal, part, rig, staticMode);
    const x = screenX(internal, internal.anchor[0] - rect.pivotX);
    const y = screenY(internal, internal.anchor[1] - rect.pivotY);
    const scale = internal.camera.scale;
    internal.ctx.drawImage(internal.page.atlas.canvas, rect.x, rect.y,
      rect.width, rect.height, x, y, rect.width * scale, rect.height * scale);
  }

  function drawParts(internal, ids, rig, context, staticMode){
    for(let index = 0; index < ids.length; index += 1){
      drawPart(internal, PART_BY_ID[ids[index]], rig, context, staticMode);
    }
  }

  function drawPixelLine(internal, x0, y0, x1, y1){
    x0 = Math.round(x0);
    y0 = Math.round(y0);
    x1 = Math.round(x1);
    y1 = Math.round(y1);
    const dx = Math.abs(x1 - x0);
    const sx = x0 < x1 ? 1 : -1;
    const dy = -Math.abs(y1 - y0);
    const sy = y0 < y1 ? 1 : -1;
    let error = dx + dy;
    const scale = internal.camera.scale;
    while(true){
      internal.ctx.fillRect(screenX(internal, x0), screenY(internal, y0), scale, scale);
      if(x0 === x1 && y0 === y1) break;
      const doubled = error * 2;
      if(doubled >= dy){ error += dy; x0 += sx; }
      if(doubled <= dx){ error += dx; y0 += sy; }
    }
  }

  function drawThickPixelLine(internal, x0, y0, x1, y1, radius, color,
      shiftX, shiftY){
    x0 = Math.round(x0 + shiftX);
    y0 = Math.round(y0 + shiftY);
    x1 = Math.round(x1 + shiftX);
    y1 = Math.round(y1 + shiftY);
    const minimums = internal.lineMinimums;
    const maximums = internal.lineMaximums;
    minimums.fill(LINE_MIN_SENTINEL);
    maximums.fill(LINE_MAX_SENTINEL);
    const dx = Math.abs(x1 - x0);
    const sx = x0 < x1 ? 1 : -1;
    const dy = -Math.abs(y1 - y0);
    const sy = y0 < y1 ? 1 : -1;
    let error = dx + dy;
    while(true){
      for(let offsetY = -radius; offsetY <= radius; offsetY += 1){
        const row = y0 + offsetY + LINE_ROW_OFFSET;
        if(row < 0 || row >= LINE_ROW_COUNT) continue;
        const edge = radius - Math.floor(Math.abs(offsetY) / 2);
        const minimum = x0 - edge;
        const maximum = x0 + edge;
        if(minimum < minimums[row]) minimums[row] = minimum;
        if(maximum > maximums[row]) maximums[row] = maximum;
      }
      if(x0 === x1 && y0 === y1) break;
      const doubled = error * 2;
      if(doubled >= dy){ error += dy; x0 += sx; }
      if(doubled <= dx){ error += dx; y0 += sy; }
    }
    internal.ctx.fillStyle = color;
    const scale = internal.camera.scale;
    const pathBatch = typeof internal.ctx.beginPath === "function" &&
      typeof internal.ctx.rect === "function" && typeof internal.ctx.fill === "function";
    if(pathBatch) internal.ctx.beginPath();
    for(let row = 0; row < LINE_ROW_COUNT; row += 1){
      const minimum = minimums[row];
      const maximum = maximums[row];
      if(minimum > maximum) continue;
      const x = screenX(internal, minimum);
      const y = screenY(internal, row - LINE_ROW_OFFSET);
      const width = (maximum - minimum + 1) * scale;
      if(pathBatch) internal.ctx.rect(x, y, width, scale);
      else internal.ctx.fillRect(x, y, width, scale);
    }
    if(pathBatch) internal.ctx.fill();
  }

  function drawLimbSegment(internal, x0, y0, x1, y1){
    drawThickPixelLine(internal, x0, y0, x1, y1, 4,
      internal.page.atlas.palette[1], 0, 0);
    drawThickPixelLine(internal, x0, y0, x1, y1, 2,
      internal.page.atlas.palette[3], 0, 0);
    drawThickPixelLine(internal, x0, y0, x1, y1, 0,
      internal.page.atlas.palette[5], -1, -1);
  }

  function drawLimbBridge(internal, rig, limbName, staticMode){
    const limb = staticMode ? STATIC_POSE.limbs[limbName] : rig.limbs[limbName];
    const rootPoint = limb.root;
    const jointPoint = limb.joint;
    const endPoint = limb.end;
    drawLimbSegment(internal, rootPoint.x, rootPoint.y, jointPoint.x, jointPoint.y);
    drawLimbSegment(internal, jointPoint.x, jointPoint.y, endPoint.x, endPoint.y);
  }

  function cableValue(rig, staticMode, index){
    return staticMode ? STATIC_POSE.cable[index] : rig.cable[index];
  }

  function drawCable(internal, rig, staticMode, firstSegment, endSegment){
    internal.ctx.fillStyle = internal.page.atlas.palette[18];
    for(let segment = firstSegment; segment < endSegment; segment += 1){
      const offset = segment * 2;
      drawPixelLine(internal, cableValue(rig, staticMode, offset),
        cableValue(rig, staticMode, offset + 1),
        cableValue(rig, staticMode, offset + 2),
        cableValue(rig, staticMode, offset + 3));
    }
  }

  function drawCableNode(internal, rig, staticMode){
    const scale = internal.camera.scale;
    const x = cableValue(rig, staticMode, 16);
    const y = cableValue(rig, staticMode, 17);
    internal.ctx.fillStyle = internal.accent.line;
    internal.ctx.fillRect(screenX(internal, x - 2), screenY(internal, y - 2),
      5 * scale, 5 * scale);
    internal.ctx.fillStyle = internal.accent.brand;
    internal.ctx.fillRect(screenX(internal, x - 1), screenY(internal, y - 1),
      3 * scale, 3 * scale);
    internal.ctx.fillStyle = internal.accent.panel;
    internal.ctx.fillRect(screenX(internal, x), screenY(internal, y), scale, scale);
  }

  function drawSuspensionDepth(internal, rig, staticMode){
    const lean = staticMode ? 0 : Math.round(rig.values[INDEX.bodyLean]);
    const lift = staticMode ? 0 : Math.round(rig.values[INDEX.bodyLift]);
    internal.ctx.fillStyle = internal.accent.line;
    internal.ctx.fillRect(screenX(internal, 97 + lean), screenY(internal, 151 - lift),
      14 * internal.camera.scale, internal.camera.scale);
  }

  function maskOrigin(internal, maskId, rig, staticMode){
    const mask = Art.MASKS[maskId];
    const poseSource = staticMode ? STATIC_POSE : rig;
    let x = mask.bounds[0];
    let y = mask.bounds[1];
    if(maskId === "contact-front-left"){
      const point = poseSource.supports["front-left"].mode === "loaded" ?
        poseSource.supports["front-left"].point : poseSource.limbs["front-left"].end;
      x += Math.round(point.x - REST_X["palm-fl"]);
      y += Math.round(point.y - REST_Y["palm-fl"]);
    }else if(maskId === "contact-front-right"){
      const point = poseSource.supports["front-right"].mode === "loaded" ?
        poseSource.supports["front-right"].point : poseSource.limbs["front-right"].end;
      x += Math.round(point.x - REST_X["palm-fr"]);
      y += Math.round(point.y - REST_Y["palm-fr"]);
    }else if(maskId === "contact-rear"){
      const limb = poseSource.supports["rear-left"].mode === "loaded" ?
        "rear-left" : "rear-right";
      const palm = limb === "rear-left" ? "palm-rl" : "palm-rr";
      const point = poseSource.supports[limb].point;
      x += Math.round(point.x - REST_X[palm]);
      y += Math.round(point.y - REST_Y[palm]);
    }else if(!staticMode){
      x += Math.round(rig.values[INDEX.bodyLean]);
      y -= Math.round(rig.values[INDEX.bodyLift]);
    }
    internal.anchor[0] = x;
    internal.anchor[1] = y;
  }

  function drawMask(internal, maskId, rig, staticMode){
    const mask = Art.MASKS[maskId];
    maskOrigin(internal, maskId, rig, staticMode);
    const originX = internal.anchor[0];
    const originY = internal.anchor[1];
    const scale = internal.camera.scale;
    internal.ctx.fillStyle = internal.page.atlas.palette[0];
    for(let index = 0; index < mask.commands.length; index += 1){
      const command = mask.commands[index];
      if(command[0] === "px"){
        internal.ctx.fillRect(screenX(internal, originX + command[2]),
          screenY(internal, originY + command[3]), scale, scale);
      }else if(command[0] === "run"){
        internal.ctx.fillRect(screenX(internal, originX + command[2]),
          screenY(internal, originY + command[3]), command[4] * scale, scale);
      }else if(command[0] === "rect"){
        internal.ctx.fillRect(screenX(internal, originX + command[2]),
          screenY(internal, originY + command[3]), command[4] * scale,
          command[5] * scale);
      }
    }
  }

  function drawContactMasks(internal, rig, context, staticMode){
    const supports = staticMode ? STATIC_POSE.supports : rig.supports;
    let count = 0;
    let bits = 0;
    if(supports["front-left"].mode === "loaded"){
      drawMask(internal, "contact-front-left", rig, staticMode);
      count += 1;
      bits |= 1;
    }
    if(supports["front-right"].mode === "loaded"){
      drawMask(internal, "contact-front-right", rig, staticMode);
      count += 1;
      bits |= 2;
    }
    if(supports["rear-left"].mode === "loaded" ||
       supports["rear-right"].mode === "loaded"){
      drawMask(internal, "contact-rear", rig, staticMode);
      count += 1;
      bits |= 4;
    }
    if(context.status === "dead" || context.contextPressure === "high"){
      drawMask(internal, "contact-belly", rig, staticMode);
      count += 1;
      bits |= 8;
    }
    internal.currentMaskBits = bits;
    return count;
  }

  function drawFineDetail(internal, rig, quality, staticMode){
    if(quality === QUALITY.ECONOMY || quality === QUALITY.STATIC) return;
    const scale = internal.camera.scale;
    for(let index = 0; index < DETAIL_CELLS.length; index += 1){
      const cell = DETAIL_CELLS[index];
      const part = PART_BY_ID[cell.part];
      globalAnchor(internal, part, rig, staticMode);
      const restX = REST_X[cell.part];
      const restY = REST_Y[cell.part];
      const worldX = cell.x + internal.anchor[0] - restX;
      const worldY = cell.y + internal.anchor[1] - restY;
      internal.ctx.drawImage(internal.page.detail, cell.x, cell.y, cell.width, cell.height,
        screenX(internal, worldX), screenY(internal, worldY),
        cell.width * scale, cell.height * scale);
    }
    if(quality !== QUALITY.FULL) return;
    internal.ctx.fillStyle = internal.page.atlas.palette[7];
    const roll = staticMode ? 0 : rig.values[INDEX.headRoll];
    for(let groupIndex = 0; groupIndex < AUTHORED_DITHER.length; groupIndex += 1){
      const group = AUTHORED_DITHER[groupIndex];
      const part = PART_BY_ID[group.part];
      globalAnchor(internal, part, rig, staticMode);
      const offsetX = internal.anchor[0] - REST_X[group.part];
      const offsetY = internal.anchor[1] - REST_Y[group.part];
      for(let index = 0; index < group.points.length; index += 2){
        const x = group.points[index] + offsetX +
          Math.round(roll * ((index >> 1) % 3 - 1));
        const y = group.points[index + 1] + offsetY;
        internal.ctx.fillRect(screenX(internal, x), screenY(internal, y), scale, scale);
      }
    }
    internal.dynamicDitherPixels = DITHER_PIXEL_COUNT;
  }

  function drawProp(internal, rig, context, staticMode){
    const prop = Art.PROPS[context.costume];
    if(!prop) return 0;
    const parent = PART_BY_ID[prop.parent];
    anchorForPart(internal, parent, rig, staticMode);
    const parentRestX = parent.bounds[0] + parent.pivot[0];
    const parentRestY = parent.bounds[1] + parent.pivot[1];
    let propAnchorX = prop.bounds[0] + prop.pivot[0] + internal.anchor[0] - parentRestX;
    let propAnchorY = prop.bounds[1] + prop.pivot[1] + internal.anchor[1] - parentRestY;
    if(!staticMode && PROP_CHANNEL[context.costume]){
      const value = rig.values[Rig.channelIndex(PROP_CHANNEL[context.costume])];
      if(context.costume === "fuego") propAnchorY -= Math.round(value * 4);
      else if(context.costume === "visor") propAnchorY += Math.round(value * 3);
      else{
        propAnchorX += Math.round(value * 3);
        propAnchorY += Math.round(Math.abs(value) * 1.5);
      }
    }
    const rect = internal.page.atlas.rects[PROP_KEYS[context.costume]];
    const scale = internal.camera.scale;
    const propX = propAnchorX - rect.pivotX;
    const propY = propAnchorY - rect.pivotY;
    internal.ctx.drawImage(internal.page.atlas.canvas, rect.x, rect.y,
      rect.width, rect.height, screenX(internal, propX),
      screenY(internal, propY), rect.width * scale,
      rect.height * scale);
    const markWidth = Math.min(5, rect.width);
    const markX = propX + Math.floor((rect.width - markWidth) / 2);
    const markY = propY + Math.min(2, rect.height - 1);
    internal.ctx.fillStyle = internal.accent.line;
    internal.ctx.fillRect(screenX(internal, markX), screenY(internal, markY),
      markWidth * scale, 2 * scale);
    internal.ctx.fillStyle = internal.accent.brand;
    internal.ctx.fillRect(screenX(internal, markX + 1), screenY(internal, markY),
      Math.max(1, markWidth - 2) * scale, scale);
    return 1;
  }

  function drawRim(internal, rig, context, staticMode){
    let paletteIndex = 15;
    if(context.status === "working") paletteIndex = 12;
    else if(context.status === "waiting") paletteIndex = 13;
    else if(context.status === "dead" || context.status === "done") paletteIndex = 14;
    const scale = internal.camera.scale;
    const rimParent = PART_BY_ID["fur-back"];
    globalAnchor(internal, rimParent, rig, staticMode);
    const rimOffsetX = internal.anchor[0] - REST_X["fur-back"];
    const rimOffsetY = internal.anchor[1] - REST_Y["fur-back"];
    internal.ctx.fillStyle = internal.page.atlas.palette[paletteIndex];
    internal.ctx.fillRect(screenX(internal, 84 + rimOffsetX),
      screenY(internal, 51 + rimOffsetY),
      2 * scale, 22 * scale);
    internal.ctx.fillRect(screenX(internal, 87 + rimOffsetX),
      screenY(internal, 48 + rimOffsetY),
      20 * scale, scale);
    if(!staticMode){
      const energy = Math.abs(rig.values[INDEX.lightKey]) +
        Math.abs(rig.values[INDEX.lightFill]) + Math.abs(rig.values[INDEX.lightRim]) +
        Math.abs(rig.values[INDEX.lightLoaded]) + Math.abs(rig.values[INDEX.lightSearching]) +
        Math.abs(rig.values[INDEX.lightVisor]);
      const extra = Math.min(8, Math.round(energy * 2));
      if(extra > 0){
        internal.ctx.fillRect(screenX(internal, 108 + rimOffsetX),
          screenY(internal, 49 + rimOffsetY),
          extra * scale, scale);
      }
      const visorGlow = Math.min(4, Math.ceil(rig.values[INDEX.lightVisor] * 4));
      if(visorGlow > 0){
        const faceParent = PART_BY_ID["face-mask"];
        globalAnchor(internal, faceParent, rig, false);
        const faceX = internal.anchor[0];
        const faceY = internal.anchor[1];
        internal.ctx.fillStyle = internal.page.atlas.palette[12];
        internal.ctx.fillRect(screenX(internal, faceX - 15 - visorGlow),
          screenY(internal, faceY - 7), visorGlow * scale, scale);
        internal.ctx.fillRect(screenX(internal, faceX + 14),
          screenY(internal, faceY - 7), visorGlow * scale, scale);
        internal.ctx.fillRect(screenX(internal, faceX - 8),
          screenY(internal, faceY + 6), 16 * scale, scale);
      }
    }
    internal.ctx.fillStyle = internal.accent.brand;
    internal.ctx.fillRect(screenX(internal, 85 + rimOffsetX),
      screenY(internal, 52 + rimOffsetY), scale, 20 * scale);
    internal.ctx.fillRect(screenX(internal, 88 + rimOffsetX),
      screenY(internal, 48 + rimOffsetY), 18 * scale, scale);
  }

  function render(renderer, rig, context, quality){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed || !validQuality(quality)) return false;
    try{
      if(!validInputs(rig, context)) return false;
      if(!internal.camera) return false;
      const staticMode = quality === QUALITY.STATIC;
      const contextChanged = visualContextChanged(internal, context);
      const poseChanged = staticMode ? 0 : scanPose(internal, rig, false);
      if(poseChanged < 0) return false;
      const contactChanged = staticMode ? false : contactSnapshotChanged(internal, rig);
      const qualityChanged = internal.quality !== quality;
      if(!internal.dirty && !contextChanged && !poseChanged && !qualityChanged){
        internal.skippedClean += 1;
        return false;
      }
      syncAccentPalette(internal, context.colors);

      const ctx = internal.ctx;
      ctx.imageSmoothingEnabled = false;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, internal.backingWidth, internal.backingHeight);
      internal.groupCounts.fill(0);
      internal.dynamicDitherPixels = 0;
      internal.contactMasks = 0;

      drawCable(internal, rig, staticMode, 0, 4);
      drawSuspensionDepth(internal, rig, staticMode);
      internal.groupCounts[0] = 2;
      drawLimbBridge(internal, rig, "front-right", staticMode);
      drawLimbBridge(internal, rig, "rear-right", staticMode);
      drawParts(internal, REAR_IDS, rig, context, staticMode);
      internal.groupCounts[1] = REAR_IDS.length;
      drawPart(internal, PART_BY_ID["fur-back"], rig, context, staticMode);
      internal.groupCounts[2] = 1;
      drawParts(internal, TORSO_IDS, rig, context, staticMode);
      internal.groupCounts[3] = TORSO_IDS.length;
      drawLimbBridge(internal, rig, "front-left", staticMode);
      drawLimbBridge(internal, rig, "rear-left", staticMode);
      drawParts(internal, FRONT_IDS, rig, context, staticMode);
      internal.groupCounts[4] = FRONT_IDS.length;
      drawParts(internal, FACE_IDS, rig, context, staticMode);
      internal.groupCounts[5] = FACE_IDS.length;
      drawParts(internal, CLAW_IDS, rig, context, staticMode);
      internal.contactMasks = drawContactMasks(internal, rig, context, staticMode);
      internal.groupCounts[6] = CLAW_IDS.length + internal.contactMasks;
      drawParts(internal, FUR_IDS, rig, context, staticMode);
      drawFineDetail(internal, rig, quality, staticMode);
      internal.groupCounts[7] = FUR_IDS.length +
        (quality === QUALITY.FULL || quality === QUALITY.BALANCED ? 1 : 0);
      internal.groupCounts[8] = drawProp(internal, rig, context, staticMode);
      drawCable(internal, rig, staticMode, 4, 8);
      drawCableNode(internal, rig, staticMode);
      internal.groupCounts[9] = 2;
      drawRim(internal, rig, context, staticMode);
      internal.groupCounts[10] = 1;

      if(!staticMode){
        if(scanPose(internal, rig, true) < 0) return false;
        if(poseChanged) internal.poseRevision += 1;
      }
      if(contactChanged || internal.currentMaskBits !== internal.lastMaskBits){
        internal.contactRevision += 1;
      }
      internal.lastMaskBits = internal.currentMaskBits;
      if(contextChanged) saveVisualContext(internal, context);
      internal.quality = quality;
      internal.detailPolicy = quality === QUALITY.FULL ? "all" :
        quality === QUALITY.BALANCED ? "merged-fine" :
        quality === QUALITY.ECONOMY ? "medium-key" : "safe-authored";
      internal.dirty = false;
      internal.renders += 1;
      return true;
    }catch(error){
      return false;
    }
  }

  function markDirty(renderer, reason){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed) return false;
    internal.manualRevision += 1;
    internal.dirty = true;
    return true;
  }

  function destroyRenderer(renderer){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed) return false;
    internal.destroyed = true;
    try{
      internal.ctx.setTransform(1, 0, 0, 1, 0, 0);
      internal.ctx.clearRect(0, 0, internal.backingWidth, internal.backingHeight);
    }catch(error){
      // Releasing retained pages and buffers remains safe after a hostile canvas teardown.
    }
    internal.pages.dark = null;
    internal.pages.light = null;
    internal.page = null;
    internal.poseSnapshot = null;
    internal.anchor = null;
    internal.groupCounts = null;
    internal.lineMinimums = null;
    internal.lineMaximums = null;
    internal.atlasBytes = 0;
    internal.retainedCacheBytes = 0;
    internal.typedArrayBytes = 0;
    return true;
  }

  function rendererDiagnostics(renderer){
    const internal = RENDERERS.get(renderer);
    if(!internal) return null;
    const camera = internal.camera ? {
      x:internal.camera.x, y:internal.camera.y,
      sourceX:internal.camera.sourceX, sourceY:internal.camera.sourceY,
      scale:internal.camera.scale, width:internal.camera.width,
      height:internal.camera.height,
    } : null;
    const groupCounts = internal.groupCounts ? Array.from(internal.groupCounts) : [];
    const decodedBytes = internal.atlasBytes + internal.typedArrayBytes +
      internal.retainedCacheBytes;
    return Object.freeze({
      destroyed:internal.destroyed,
      quality:internal.quality,
      detailPolicy:internal.detailPolicy,
      camera:Object.freeze(camera),
      occlusionGroups:OCCLUSION_GROUPS,
      lastGroupDrawCounts:Object.freeze(groupCounts),
      renders:internal.renders,
      skippedClean:internal.skippedClean,
      atlasBuilds:internal.atlasBuilds,
      atlasBytes:internal.atlasBytes,
      typedArrayBytes:internal.typedArrayBytes,
      retainedCacheBytes:internal.retainedCacheBytes,
      decodedBytes,
      dynamicDitherPixels:internal.dynamicDitherPixels,
      contactMasks:internal.contactMasks,
      cameraRevision:internal.cameraRevision,
      paletteRevision:internal.paletteRevision,
      accentRevision:internal.accentRevision,
      accentDescriptor:ACCENT_DESCRIPTOR,
      poseRevision:internal.poseRevision,
      contextRevision:internal.contextRevision,
      contactRevision:internal.contactRevision,
      propRevision:internal.propRevision,
      manualRevision:internal.manualRevision,
    });
  }

  NS.Renderer = Object.freeze({QUALITY, createRenderer, setViewport, setTheme,
    render, markDirty, destroyRenderer, rendererDiagnostics});
})(typeof window !== "undefined" ? window : globalThis);
