(function(root){
  "use strict";
  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  if(!NS.Core) throw new Error("ComandOSPerezOS.Core must load before Art");

  const WORLD = Object.freeze({width:224, height:192});
  const BODY_IDS = Object.freeze([
    "pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid", "neck-upper",
    "skull", "face-mask", "muzzle", "jaw", "nose", "eye-left", "eye-right",
    "lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
    "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl",
    "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr",
    "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl",
    "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr",
    "claw-front-left-1", "claw-front-left-2", "claw-front-left-3",
    "claw-front-right-1", "claw-front-right-2", "claw-front-right-3",
    "claw-rear-left-1", "claw-rear-left-2", "claw-rear-left-3",
    "claw-rear-right-1", "claw-rear-right-2", "claw-rear-right-3",
    "fur-back", "fur-belly", "fur-head",
  ]);

  const PALETTE = Object.freeze([
    "#211713", "#382219", "#53301f", "#70432a", "#925e38", "#b9824f",
    "#d8aa70", "#f1d59a", "#171b1e", "#d79a2b", "#efe0bd", "#9f4d43",
    "#65d6bd", "#ffd45c", "#e86b55", "#72d7f0", "#ec8736", "#ffe27a",
    "#718392", "#57242c", "#fff4d6",
  ]);
  const LIGHT_PALETTE = Object.freeze([
    "#32221d", "#382219", "#53301f", "#70432a", "#925e38", "#b9824f",
    "#d8aa70", "#f1d59a", "#171b1e", "#d79a2b", "#efe0bd", "#9f4d43",
    "#48cbb0", "#f0ba38", "#e75b49", "#53cce9", "#ed762a", "#ffda55",
    "#7d929f", "#682a34", "#fff4d6",
  ]);
  const PALETTE_ROLES = Object.freeze({
    stableIndices:Object.freeze([1,2,3,4,5,6,7,8,9,10,11,20]),
    variableIndices:Object.freeze([0,12,13,14,15,16,17,18,19]),
    roleByIndex:Object.freeze([
      "deep-shadow", "brown-ink", "brown-dark", "brown-mid", "brown-warm",
      "brown-light", "brown-tan", "brown-highlight", "eye-ink", "eye-amber",
      "bone", "mouth", "loaded-light", "searching-light", "turned-light",
      "visor-light", "fire", "fire-light", "cable-metal", "small-prop",
      "specular",
    ]),
  });
  const THEMES = Object.freeze({dark:PALETTE, light:LIGHT_PALETTE});

  const freezeCommands = commands => Object.freeze(commands.map(command => Object.freeze(command)));
  function replaceState(commands){ return {replace:commands}; }
  function freezeStates(states, baseCommands){
    const frozen = Object.create(null);
    for(const name of Object.keys(states || {})){
      const state = states[name];
      frozen[name] = freezeCommands(state && state.replace ? state.replace : [...baseCommands, ...state]);
    }
    return Object.freeze(frozen);
  }
  function piece(id, parent, pivot, bounds, z, commands, states){
    return Object.freeze({id, parent, pivot:Object.freeze(pivot), bounds:Object.freeze(bounds), z,
      commands:freezeCommands(commands), states:freezeStates(states, commands)});
  }

  const PARTS = Object.freeze([
    piece("pelvis", "world", [18,15], [88,116,36,28], 20, [
      ["rect",2,3,6,30,15], ["rect",2,7,3,22,22], ["rect",3,5,5,26,17],
      ["rect",4,8,4,20,18], ["rect",5,11,6,14,13], ["run",6,13,7,9],
      ["poly",1,3,12,8,3,7,23], ["poly",1,28,4,33,13,29,23],
      ["run",2,8,24,20], ["px",7,24,9],
    ], {turned:[["rect",14,4,12,4,6],["run",13,8,10,20]]}),
    piece("abdomen", "pelvis", [18,24], [89,88,36,34], 18, [
      ["rect",2,3,6,30,23], ["rect",2,6,3,24,29], ["rect",3,5,5,26,25],
      ["rect",4,8,4,20,26], ["rect",5,11,7,14,20], ["run",6,13,8,10],
      ["poly",1,3,13,8,3,6,28], ["poly",1,28,4,33,14,29,29],
      ["run",2,7,31,22], ["px",7,23,11],
    ], {loaded:[["rect",12,12,10,12,9],["run",15,14,12,8]]}),
    piece("ribcage", "abdomen", [23,31], [83,53,47,40], 19, [
      ["rect",2,3,10,41,22], ["rect",2,7,4,33,34], ["rect",3,5,7,37,28],
      ["rect",4,9,5,29,31], ["rect",5,12,8,23,24], ["rect",6,16,10,15,18],
      ["poly",1,3,16,9,3,6,31], ["poly",1,38,4,44,17,40,32],
      ["run",5,12,28,23], ["run",2,8,37,31], ["px",7,31,12],
      ["px",6,13,20], ["px",5,35,24],
    ], {loaded:[["rect",12,14,12,18,11],["run",15,17,14,12]]}),
    piece("neck-lower", "ribcage", [10,20], [102,38,21,25], 22, [
      ["rect",1,1,3,19,20], ["rect",2,3,1,15,23], ["rect",3,4,3,13,20],
      ["rect",4,6,4,10,17], ["rect",5,7,5,8,14], ["run",6,8,6,6],
      ["run",2,3,23,15],
    ]),
    piece("neck-mid", "neck-lower", [9,17], [103,25,19,21], 23, [
      ["rect",1,1,2,17,18], ["rect",2,3,1,14,20], ["rect",3,4,3,12,17],
      ["rect",4,6,4,9,14], ["rect",5,7,5,7,12], ["run",6,8,6,5],
      ["px",7,12,7], ["px",2,3,16],
    ], {searching:[["run",13,6,4,8],["px",17,9,6]]}),
    piece("neck-upper", "neck-mid", [10,15], [102,14,21,19], 24, [
      ["rect",1,1,2,19,16], ["rect",2,3,1,15,18], ["rect",3,4,3,13,15],
      ["rect",4,6,4,10,12], ["rect",5,7,5,8,10], ["run",6,8,6,6],
      ["run",2,3,17,15],
    ]),
    piece("skull", "neck-upper", [31,29], [80,3,66,58], 30, [
      ["rect",0,10,8,46,43], ["rect",1,6,14,54,31], ["rect",1,14,4,38,51],
      ["rect",2,9,9,48,40], ["rect",3,12,7,42,43], ["rect",4,15,10,36,37],
      ["rect",5,19,12,28,31], ["run",6,22,13,22], ["run",2,14,52,38],
      ["px",7,43,14], ["px",6,18,21], ["px",2,54,24],
    ], {searching:[
          ["poly",0,0,18,9,6,7,27], ["poly",0,55,7,65,18,57,29],
          ["run",13,14,17,38], ["run",17,20,20,26], ["px",13,8,29],
        ],
        turned:[
          ["poly",0,3,9,19,2,14,20], ["poly",0,49,5,64,16,54,26],
          ["rect",14,44,15,12,16], ["run",13,39,19,18], ["px",14,58,22],
        ]}),
    piece("face-mask", "skull", [29,10], [84,12,58,38], 34, [
      ["rect",6,7,10,44,20], ["rect",6,12,5,34,30], ["rect",7,11,8,36,24],
      ["rect",7,16,6,26,28], ["poly",1,7,13,24,8,20,27],
      ["poly",1,34,8,51,13,38,27], ["poly",2,10,16,24,10,20,28],
      ["poly",2,33,10,48,16,38,28], ["run",6,17,32,24], ["px",20,40,12],
    ], {searching:[["rect",13,10,7,24,5],["run",17,14,8,16]]}),
    piece("muzzle", "face-mask", [22,9], [90,18,45,28], 38, [
      ["rect",5,8,9,29,13], ["rect",6,11,6,23,19], ["rect",7,15,8,15,15],
      ["poly",6,8,13,18,5,18,23], ["poly",6,27,5,37,13,27,23],
      ["run",5,12,22,21], ["run",2,10,24,25], ["px",20,29,11],
    ], {turned:[["rect",14,4,7,5,5],["run",13,9,5,20]]}),
    piece("jaw", "muzzle", [21,5], [92,29,43,20], 37, [
      ["run",1,15,5,13], ["run",11,16,6,11], ["run",11,17,7,9],
      ["run",20,18,7,7], ["px",20,15,6], ["px",20,27,6],
    ], {loaded:[["rect",11,7,2,21,9],["run",20,10,3,15]]}),
    piece("nose", "muzzle", [8,5], [104,19,17,12], 42, [
      ["rect",8,5,3,7,6], ["rect",8,4,4,9,4], ["rect",8,6,2,5,8],
      ["run",10,6,4,5], ["px",20,7,4], ["px",0,10,7],
    ], {searching:[["run",13,3,2,9],["px",17,9,3]]}),
    piece("eye-left", "face-mask", [6,5], [101,10,12,11], 44, [
      ["rect",8,3,4,6,4], ["rect",9,4,4,4,3], ["rect",20,5,4,2,1],
      ["px",0,7,6], ["run",1,4,8,4],
    ], {loaded:[["rect",12,2,2,5,5],["px",20,3,2]],
        searching:[["rect",13,2,2,5,5],["px",20,5,2]]}),
    piece("eye-right", "face-mask", [6,5], [117,10,12,11], 44, [
      ["rect",8,3,4,6,4], ["rect",9,4,4,4,3], ["rect",20,6,4,2,1],
      ["px",0,4,6], ["run",1,4,8,4],
    ], {loaded:[["rect",12,2,2,5,5],["px",20,5,2]],
        searching:[["rect",13,2,2,5,5],["px",20,3,2]]}),
    piece("lid-left-upper", "eye-left", [4,6], [103,10,9,7], 46, [
      ["run",1,1,3,7], ["run",3,2,2,5], ["run",5,3,2,3],
    ], {turned:[["rect",14,1,2,7,2]]}),
    piece("lid-left-lower", "eye-left", [4,1], [103,17,9,5], 46, [
      ["run",1,1,1,7], ["run",3,2,2,5], ["run",5,3,2,3],
    ]),
    piece("lid-right-upper", "eye-right", [4,6], [119,10,9,7], 46, [
      ["run",1,1,3,7], ["run",3,2,2,5], ["run",5,3,2,3],
    ], {turned:[["rect",14,1,2,7,2]]}),
    piece("lid-right-lower", "eye-right", [4,1], [119,17,9,5], 46, [
      ["run",1,1,1,7], ["run",3,2,2,5], ["run",5,3,2,3],
    ]),
    piece("arm-fl-upper", "ribcage", [6,6], [64,65,20,34], 28, [
      ["poly",1,2,2,18,5,6,32], ["rect",1,2,3,16,28], ["rect",1,0,12,5,8],
      ["rect",2,4,5,12,24],
      ["poly",3,6,4,16,8,8,29], ["rect",5,8,7,6,17],
      ["run",6,9,8,5], ["run",4,6,25,7], ["px",7,13,10], ["px",2,5,18],
    ], {loaded:[["rect",12,6,10,8,8]]}),
    piece("arm-fl-fore", "arm-fl-upper", [7,5], [57,92,19,33], 29, [
      ["poly",1,3,1,16,4,7,32], ["rect",1,2,2,15,29], ["rect",1,0,18,4,8],
      ["rect",2,4,6,11,22],
      ["poly",3,6,4,15,7,8,29], ["rect",4,7,8,7,16],
      ["run",6,8,9,5], ["run",5,7,25,6], ["px",7,12,11], ["px",2,5,17],
    ]),
    piece("wrist-fl", "arm-fl-fore", [7,4], [54,120,18,15], 30, [
      ["poly",1,1,3,16,1,14,12], ["rect",1,1,1,16,13], ["rect",1,0,0,2,15],
      ["rect",1,0,7,4,5],
      ["rect",3,3,3,11,9],
      ["poly",5,5,4,13,3,8,11], ["run",6,7,5,5], ["run",2,3,12,10],
    ]),
    piece("palm-fl", "wrist-fl", [11,4], [43,131,27,18], 31, [
      ["poly",1,1,5,25,2,21,16], ["rect",1,1,1,25,15], ["rect",1,0,0,2,18],
      ["rect",1,0,9,5,6],
      ["rect",2,3,5,20,10],
      ["poly",3,5,3,23,4,17,14], ["rect",5,7,6,13,7],
      ["run",6,9,7,8], ["run",4,5,14,14], ["px",7,17,7],
    ], {loaded:[["rect",12,8,5,12,6]]}),
    piece("arm-fr-upper", "ribcage", [14,6], [130,65,20,34], 12, [
      ["poly",1,2,5,18,1,15,19], ["poly",1,7,15,17,12,9,32], ["rect",1,2,3,16,28],
      ["rect",1,15,4,5,8],
      ["rect",2,5,5,11,11], ["rect",3,8,13,8,15], ["rect",5,9,7,6,9],
      ["run",6,10,8,4], ["run",4,10,24,5], ["px",7,14,15], ["px",2,8,20],
    ], {loaded:[["rect",12,5,10,8,8]], turned:replaceState([
      ["rect",1,2,4,16,14], ["rect",3,4,2,13,17], ["rect",4,7,5,11,12],
      ["rect",2,8,17,10,14], ["rect",5,10,19,7,10], ["run",6,11,20,5],
      ["px",7,15,21],
    ])}),
    piece("arm-fr-fore", "arm-fr-upper", [12,5], [142,92,19,33], 13, [
      ["poly",1,2,4,17,1,14,17], ["poly",1,6,14,16,11,8,31], ["rect",1,2,2,15,29],
      ["rect",1,15,5,4,8],
      ["rect",2,4,5,11,10], ["rect",3,7,12,8,14], ["rect",4,8,7,6,9],
      ["run",6,9,8,4], ["run",5,9,23,5], ["px",7,13,14], ["px",2,7,19],
    ], {turned:replaceState([
      ["rect",1,2,3,15,14], ["rect",3,1,6,16,12], ["rect",4,3,8,13,9],
      ["rect",2,4,15,12,16], ["rect",5,6,18,9,11], ["run",6,8,19,6],
      ["px",7,13,20],
    ])}),
    piece("wrist-fr", "arm-fr-fore", [10,4], [148,120,18,15], 14, [
      ["poly",1,1,2,16,4,4,13], ["rect",1,1,1,16,13], ["rect",1,16,0,2,15],
      ["rect",1,14,2,4,5],
      ["rect",2,4,3,12,8],
      ["poly",3,6,4,15,5,8,12], ["rect",5,7,5,7,6],
      ["run",6,8,6,5], ["run",4,5,12,9], ["px",7,12,7],
    ], {turned:replaceState([
      ["rect",1,1,3,16,9], ["rect",3,3,2,14,10], ["rect",5,6,4,10,7],
      ["run",6,8,5,6], ["run",2,3,12,13],
    ])}),
    piece("palm-fr", "wrist-fr", [15,4], [151,131,27,18], 15, [
      ["poly",1,2,2,26,6,5,16], ["rect",1,1,1,25,15], ["rect",1,25,0,2,18],
      ["rect",1,22,2,5,6],
      ["rect",2,5,4,20,10],
      ["poly",3,7,4,24,6,9,14], ["rect",5,9,6,13,7],
      ["run",6,10,7,8], ["run",4,7,14,13], ["px",7,19,8],
    ], {loaded:[["rect",12,7,5,12,6]], turned:replaceState([
      ["rect",1,2,2,20,14], ["rect",3,5,1,19,12], ["rect",5,8,3,16,9],
      ["poly",2,16,4,26,8,17,16], ["run",6,10,4,9], ["px",7,21,5],
    ])}),
    piece("leg-rl-upper", "pelvis", [8,5], [76,131,22,31], 16, [
      ["poly",1,2,2,20,5,7,30], ["rect",1,2,2,18,27], ["rect",1,0,14,5,8],
      ["rect",2,4,5,14,20],
      ["poly",3,6,4,18,8,9,28], ["rect",4,8,7,8,16],
      ["run",6,9,8,6], ["run",5,8,24,7], ["px",7,14,11], ["px",2,6,18],
    ]),
    piece("leg-rl-lower", "leg-rl-upper", [9,4], [72,154,21,29], 17, [
      ["poly",1,3,1,18,4,8,28], ["rect",1,2,1,17,27], ["rect",1,0,12,5,8],
      ["rect",2,5,5,12,18],
      ["poly",3,7,4,17,7,10,26], ["rect",5,9,7,7,14],
      ["run",6,10,8,5], ["run",4,8,22,6], ["px",7,14,10], ["px",2,7,17],
    ]),
    piece("ankle-rl", "leg-rl-lower", [9,3], [70,176,20,12], 18, [
      ["poly",1,1,3,18,1,15,10], ["rect",1,1,1,18,10], ["rect",1,0,0,2,12],
      ["rect",1,0,6,4,5],
      ["rect",3,4,3,12,7],
      ["poly",5,6,4,14,3,9,10], ["run",6,8,5,5], ["run",2,4,10,11],
    ]),
    piece("palm-rl", "ankle-rl", [14,4], [54,180,31,12], 19, [
      ["poly",1,1,3,30,2,24,11], ["rect",1,1,1,29,10], ["rect",1,0,0,2,12],
      ["rect",1,0,6,6,5],
      ["rect",2,4,4,24,7],
      ["poly",3,6,3,27,4,20,10], ["rect",5,9,5,15,5],
      ["run",6,11,6,9], ["run",4,7,10,14], ["px",7,21,6],
    ]),
    piece("leg-rr-upper", "pelvis", [14,5], [119,131,22,31], 14, [
      ["poly",1,2,5,20,2,16,18], ["poly",1,7,15,19,12,10,30], ["rect",1,2,2,18,27],
      ["rect",1,17,4,5,8],
      ["rect",2,5,6,13,10], ["rect",3,8,13,9,14], ["rect",4,9,8,7,9],
      ["run",6,10,9,5], ["run",5,11,23,5], ["px",7,15,14], ["px",2,8,20],
    ], {turned:replaceState([
      ["rect",1,3,2,16,14], ["rect",3,5,3,14,13], ["rect",4,7,5,11,10],
      ["rect",2,7,14,12,14], ["rect",5,9,16,9,11], ["run",6,11,17,5],
    ])}),
    piece("leg-rr-lower", "leg-rr-upper", [12,4], [129,154,21,29], 15, [
      ["poly",1,2,4,19,1,15,16], ["poly",1,7,13,18,10,9,28], ["rect",1,2,1,17,27],
      ["rect",1,16,4,5,8],
      ["rect",2,5,5,12,9], ["rect",3,8,11,9,13], ["rect",5,9,7,7,8],
      ["run",6,10,8,5], ["run",4,10,21,5], ["px",7,14,13], ["px",2,8,18],
    ], {turned:replaceState([
      ["rect",1,2,2,16,13], ["rect",3,4,3,14,12], ["rect",5,6,5,10,9],
      ["rect",2,5,13,13,13], ["rect",4,7,15,10,10], ["run",6,9,16,6],
    ])}),
    piece("ankle-rr", "leg-rr-lower", [11,3], [135,176,20,12], 16, [
      ["poly",1,1,2,19,4,5,11], ["rect",1,1,1,18,10], ["rect",1,18,0,2,12],
      ["rect",1,16,1,4,5],
      ["rect",2,5,3,13,7],
      ["poly",3,7,4,17,5,10,10], ["rect",5,9,5,7,5],
      ["run",6,10,6,5], ["run",4,6,10,10], ["px",7,14,6],
    ], {turned:replaceState([
      ["rect",1,1,2,17,8], ["rect",3,3,1,14,9], ["rect",5,6,3,10,6],
      ["run",6,8,4,6], ["run",2,4,10,13],
    ])}),
    piece("palm-rr", "ankle-rr", [17,4], [139,180,31,12], 17, [
      ["poly",1,2,2,30,5,7,11], ["rect",1,1,1,29,10], ["rect",1,29,0,2,12],
      ["rect",1,25,1,6,5],
      ["rect",2,6,4,23,7],
      ["poly",3,8,4,28,6,11,10], ["rect",5,11,6,15,5],
      ["run",6,12,7,9], ["run",4,9,10,13], ["px",7,23,7],
    ], {turned:replaceState([
      ["rect",1,2,1,24,9], ["rect",3,5,2,22,8], ["rect",5,8,3,17,6],
      ["poly",2,20,2,30,5,21,10], ["run",6,10,4,11], ["px",7,24,4],
    ])}),
    piece("claw-front-left-1", "palm-fl", [8,1], [38,145,12,7], 35, [
      ["poly",0,1,1,11,2,3,6], ["poly",10,2,2,9,2,3,5], ["px",20,3,3],
    ], {loaded:replaceState([["poly",0,1,1,11,1,6,6],["poly",10,2,2,10,2,6,5],["run",12,3,4,6],["px",20,7,3]])}),
    piece("claw-front-left-2", "palm-fl", [7,1], [48,147,11,7], 35, [
      ["poly",0,1,1,10,2,2,6], ["poly",10,2,2,8,2,2,5], ["px",20,3,3],
    ], {loaded:replaceState([["poly",0,1,1,10,1,5,6],["poly",10,2,2,9,2,5,5],["run",12,3,4,5],["px",20,6,3]])}),
    piece("claw-front-left-3", "palm-fl", [6,1], [58,146,10,7], 35, [
      ["poly",0,1,1,9,2,2,6], ["poly",10,2,2,7,2,2,5], ["px",20,3,3],
    ], {loaded:replaceState([["poly",0,1,1,9,1,4,6],["poly",10,2,2,8,2,4,5],["run",12,2,4,5],["px",20,5,3]])}),
    piece("claw-front-right-1", "palm-fr", [3,1], [169,145,12,7], 35, [
      ["poly",0,1,2,11,1,9,6], ["poly",10,3,2,10,2,9,5], ["px",20,8,3],
    ], {loaded:replaceState([["poly",0,1,1,11,1,6,6],["poly",10,2,2,10,2,6,5],["run",12,4,4,6],["px",20,5,3]])}),
    piece("claw-front-right-2", "palm-fr", [3,1], [159,147,11,7], 35, [
      ["poly",0,1,2,10,1,9,6], ["poly",10,3,2,9,2,9,5], ["px",20,7,3],
    ], {loaded:replaceState([["poly",0,1,1,10,1,5,6],["poly",10,2,2,9,2,5,5],["run",12,4,4,5],["px",20,5,3]])}),
    piece("claw-front-right-3", "palm-fr", [3,1], [150,146,10,7], 35, [
      ["poly",0,1,2,9,1,8,6], ["poly",10,3,2,8,2,8,5], ["px",20,6,3],
    ], {loaded:replaceState([["poly",0,1,1,9,1,4,6],["poly",10,2,2,8,2,4,5],["run",12,3,4,5],["px",20,4,3]])}),
    piece("claw-rear-left-1", "palm-rl", [8,1], [48,186,12,6], 23, [
      ["poly",0,1,1,5,1,2,5], ["poly",10,2,2,4,2,2,4],
      ["rect",6,1,3,2,2], ["px",20,3,2],
    ], {loaded:replaceState([["poly",0,1,1,11,1,6,5],["poly",10,2,2,10,2,6,4],["run",12,3,3,6],["px",20,7,2]])}),
    piece("claw-rear-left-2", "palm-rl", [7,1], [58,187,11,5], 23, [
      ["poly",0,1,1,7,1,2,4], ["poly",10,2,2,6,2,2,3],
      ["run",6,2,3,3], ["px",20,3,2],
    ], {loaded:replaceState([["poly",0,1,1,10,1,5,4],["poly",10,2,2,9,2,5,3],["run",12,3,3,5],["px",20,6,2]])}),
    piece("claw-rear-left-3", "palm-rl", [6,1], [68,187,10,5], 23, [
      ["poly",0,1,1,6,1,2,4], ["poly",10,2,2,5,2,2,3],
      ["run",6,2,3,3], ["px",20,3,2],
    ], {loaded:replaceState([["poly",0,1,1,9,1,4,4],["poly",10,2,2,8,2,4,3],["run",12,2,3,5],["px",20,5,2]])}),
    piece("claw-rear-right-1", "palm-rr", [3,1], [165,186,12,6], 23, [
      ["poly",0,4,1,11,1,10,5], ["poly",10,5,2,10,2,10,4],
      ["run",6,7,3,3], ["px",20,9,2],
    ], {loaded:replaceState([["poly",0,1,1,8,1,2,5],["poly",10,2,2,7,2,2,4],["rect",12,1,3,5,2],["px",20,3,2]])}),
    piece("claw-rear-right-2", "palm-rr", [3,1], [155,187,11,5], 23, [
      ["poly",0,4,1,10,1,9,4], ["poly",10,5,2,9,2,9,3],
      ["run",6,6,3,3], ["px",20,8,2],
    ], {loaded:replaceState([["poly",0,1,1,10,1,5,4],["poly",10,2,2,9,2,5,3],["run",12,4,3,5],["px",20,5,2]])}),
    piece("claw-rear-right-3", "palm-rr", [3,1], [146,187,10,5], 23, [
      ["poly",0,4,1,9,1,8,4], ["poly",10,5,2,8,2,8,3],
      ["run",6,6,3,2], ["px",20,7,2],
    ], {loaded:replaceState([["poly",0,1,1,9,1,4,4],["poly",10,2,2,8,2,4,3],["run",12,3,3,5],["px",20,4,2]])}),
    piece("fur-back", "ribcage", [34,33], [60,40,100,112], 10, [
      ["rect",1,9,30,82,61], ["rect",1,16,17,70,84], ["rect",1,24,10,54,96],
      ["rect",2,12,25,76,68], ["rect",2,20,15,62,89],
      ["rect",3,17,23,66,70], ["rect",3,25,16,50,84],
      ["rect",4,22,27,56,62], ["rect",4,29,20,42,75],
      ["rect",5,30,25,37,62], ["run",6,34,27,28],
      ["poly",1,5,38,16,24,15,57], ["poly",1,84,24,96,42,84,58],
      ["poly",2,13,81,29,99,18,108], ["poly",2,70,91,86,78,79,108],
      ["run",5,18,36,17], ["run",6,62,31,14], ["run",2,16,94,20],
      ["px",7,63,36], ["px",6,28,53], ["px",5,74,69],
    ]),
    piece("fur-belly", "abdomen", [31,37], [77,74,66,72], 25, [
      ["rect",3,13,9,40,8], ["rect",3,9,17,48,24], ["rect",3,13,41,40,12],
      ["rect",4,16,12,34,8], ["rect",4,12,20,42,20], ["rect",4,16,40,34,14],
      ["rect",5,20,15,26,8], ["rect",5,16,23,34,18], ["rect",5,20,41,26,12],
      ["rect",6,23,19,20,8], ["rect",6,20,27,26,15], ["rect",6,24,42,18,10],
      ["rect",7,27,22,12,8], ["rect",7,24,30,18,12], ["rect",7,27,42,12,8],
      ["poly",3,4,24,15,9,13,46], ["poly",3,51,10,62,25,51,48],
      ["poly",4,10,51,25,66,18,70], ["poly",4,43,52,57,46,49,68],
      ["run",6,21,31,24], ["run",7,27,34,12], ["run",5,20,56,26],
      ["px",7,42,27], ["px",6,23,35], ["px",5,48,40],
    ], {loaded:[["poly",12,8,8,26,8,17,25],["run",15,12,11,10]]}),
    piece("fur-head", "skull", [31,28], [76,0,72,60], 32, [
      ["poly",1,3,19,18,3,16,15], ["poly",1,13,7,28,0,26,12],
      ["poly",2,25,5,38,0,37,12], ["poly",1,37,2,53,5,48,15],
      ["poly",2,49,7,68,18,55,21], ["poly",1,55,20,71,31,55,34],
      ["poly",2,4,32,18,27,14,43], ["poly",1,10,43,23,51,17,57],
      ["poly",2,49,43,63,37,56,55], ["poly",1,30,51,42,57,34,59],
      ["run",5,20,7,18], ["run",6,39,8,12], ["run",4,15,34,9],
      ["px",7,45,10], ["px",6,22,17], ["px",5,57,27],
    ], {searching:[["run",13,15,9,24],["px",17,28,7]]}),
  ]);

  function prop(id, parent, pivot, bounds, z, commands){
    return piece(id, parent, pivot, bounds, z, commands);
  }
  const PROPS = Object.freeze({
    corona:prop("corona", "skull", [17,17], [94,-14,35,19], 60, [
      ["poly",9,2,16,5,2,11,15], ["poly",13,10,15,17,0,22,15],
      ["poly",9,21,15,29,3,32,16], ["rect",16,4,13,27,4], ["run",17,7,14,21],
    ]),
    casco:prop("casco", "skull", [25,30], [85,-2,52,34], 58, [
      ["rect",8,4,8,44,22], ["rect",18,7,5,38,23], ["rect",3,11,3,30,21],
      ["run",6,14,5,24], ["rect",13,23,1,6,4],
    ]),
    visor:prop("visor", "face-mask", [22,10], [91,8,45,18], 62, [
      ["rect",8,2,3,41,12], ["rect",15,5,4,35,9], ["run",20,9,5,18],
      ["run",12,7,11,28], ["px",17,35,6],
    ]),
    fuego:prop("fuego", "fur-back", [10,31], [69,42,24,35], 8, [
      ["poly",14,3,33,8,4,13,32], ["poly",16,8,32,14,0,20,33],
      ["poly",17,11,31,15,10,18,31], ["run",13,9,29,10],
    ]),
    hamster:prop("hamster", "palm-fl", [10,18], [47,112,23,23], 56, [
      ["rect",1,3,5,17,15], ["rect",4,5,3,13,17], ["rect",6,7,5,9,12],
      ["px",8,8,8], ["px",8,14,8], ["rect",11,10,11,3,2],
    ]),
    gordo:prop("gordo", "abdomen", [20,17], [87,82,41,38], 54, [
      ["rect",1,3,4,35,29], ["rect",4,6,2,29,32], ["rect",5,9,5,23,26],
      ["rect",6,12,8,17,20], ["run",7,15,10,11], ["run",2,6,33,29],
    ]),
    huevo:prop("huevo", "palm-fr", [10,16], [153,114,22,21], 56, [
      ["rect",10,4,4,14,14], ["rect",20,6,2,10,16], ["rect",7,8,4,6,12],
      ["run",6,7,16,8], ["px",13,12,6],
    ]),
    bufanda:prop("bufanda", "neck-mid", [22,11], [91,25,45,28], 52, [
      ["rect",19,2,3,41,10], ["rect",14,5,1,35,11], ["run",17,9,3,24],
      ["poly",19,27,11,42,12,37,27], ["poly",14,30,10,40,13,35,24],
    ]),
  });
  const ATLAS_KEYS = Object.freeze([
    ...BODY_IDS,
    ...PARTS.flatMap(part => Object.keys(part.states).map(state => `${part.id}@${state}`)),
    ...Object.keys(PROPS).map(name => `prop:${name}`),
  ]);
  const REQUIRED_STATE_KEYS = Object.freeze(ATLAS_KEYS.filter(key => key.includes("@")));
  const PROP_NAMES = Object.freeze(["corona", "casco", "visor", "fuego",
    "hamster", "gordo", "huevo", "bufanda"]);
  const MASK_NAMES = Object.freeze(["contact-belly", "contact-front-left",
    "contact-front-right", "contact-ground"]);

  function mask(id, bounds, commands){
    return Object.freeze({id, bounds:Object.freeze(bounds), commands:freezeCommands(commands)});
  }
  const MASKS = Object.freeze({
    "contact-belly":mask("contact-belly", [96,103,27,17], [
      ["rect",0,2,2,23,13], ["run",0,5,15,17],
    ]),
    "contact-front-left":mask("contact-front-left", [44,137,25,12], [
      ["rect",0,1,1,23,10], ["run",0,4,11,17],
    ]),
    "contact-front-right":mask("contact-front-right", [152,137,25,12], [
      ["rect",0,1,1,23,10], ["run",0,4,11,17],
    ]),
    "contact-ground":mask("contact-ground", [53,180,118,12], [
      ["rect",0,1,3,116,8], ["run",0,8,2,102],
    ]),
  });

  const CAMERAS = Object.freeze({
    full:Object.freeze({x:0, y:0, width:224, height:192}),
    compact:Object.freeze({x:22, y:8, width:180, height:148}),
    portrait:Object.freeze({x:48, y:0, width:128, height:160}),
    face:Object.freeze({x:78, y:0, width:72, height:64}),
  });

  function compactCamera(stageWidth, stageHeight){
    stageWidth = Math.max(0, Math.floor(Number(stageWidth) || 0));
    stageHeight = Math.max(0, Math.floor(Number(stageHeight) || 0));
    const fullScale = Math.floor(Math.min(stageWidth / WORLD.width,
                                          stageHeight / WORLD.height));
    const fullFits = fullScale >= 1;
    const scale = fullFits ? fullScale : 1;
    const width = fullFits ? WORLD.width : Math.min(stageWidth, CAMERAS.compact.width);
    const height = fullFits ? WORLD.height : Math.min(stageHeight, CAMERAS.compact.height);
    const sourceX = fullFits ? 0 : CAMERAS.compact.x + Math.floor((CAMERAS.compact.width - width) / 2);
    const sourceY = fullFits ? 0 : CAMERAS.compact.y + Math.floor((CAMERAS.compact.height - height) / 2);
    return Object.freeze({
      x:Math.floor((stageWidth - width * scale) / 2),
      y:Math.floor((stageHeight - height * scale) / 2),
      sourceX, sourceY, scale, width, height,
    });
  }

  const COMMAND_LENGTHS = Object.freeze({px:4, run:5, rect:6, poly:8});
  const RASTER_LIMIT = 1024;
  function commandError(command, width, height, paletteLength){
    if(!Array.isArray(command) || COMMAND_LENGTHS[command[0]] !== command.length) return "shape";
    if(!command.slice(1).every(Number.isInteger)) return "integer";
    if(command[1] < 0 || command[1] >= paletteLength) return "palette";
    const kind = command[0];
    if(kind === "px") return command[2] < 0 || command[3] < 0 || command[2] >= width || command[3] >= height ? "bounds" : "";
    if(kind === "run") return command[4] < 1 || command[2] < 0 || command[3] < 0 || command[2] + command[4] > width || command[3] >= height ? "bounds" : "";
    if(kind === "rect") return command[4] < 1 || command[5] < 1 || command[2] < 0 || command[3] < 0 || command[2] + command[4] > width || command[3] + command[5] > height ? "bounds" : "";
    for(let i = 2; i < 8; i += 2){
      if(command[i] < 0 || command[i + 1] < 0 || command[i] >= width || command[i + 1] >= height) return "bounds";
    }
    return "";
  }

  function validatePiece(item, knownParents, errors, prefix, paletteLength, checkWorldBounds){
    if(!item || typeof item !== "object" || typeof item.id !== "string"){
      errors.push(`${prefix}: invalid piece`);
      return;
    }
    const label = `${prefix}.${item.id}`;
    if(typeof item.parent !== "string" || !knownParents.has(item.parent)) errors.push(`${label}: invalid parent ${item.parent}`);
    const pivotValid = Array.isArray(item.pivot) && item.pivot.length === 2 && item.pivot.every(Number.isInteger);
    const boundsValid = Array.isArray(item.bounds) && item.bounds.length === 4 &&
      item.bounds.every(Number.isInteger) && item.bounds[2] > 0 && item.bounds[3] > 0;
    if(!pivotValid) errors.push(`${label}: invalid pivot`);
    if(!boundsValid) errors.push(`${label}: invalid bounds`);
    if(boundsValid && checkWorldBounds && (item.bounds[0] < 0 || item.bounds[1] < 0 ||
        item.bounds[0] + item.bounds[2] > WORLD.width ||
        item.bounds[1] + item.bounds[3] > WORLD.height)){
      errors.push(`${label}: world bounds exceeded`);
    }
    if(!Number.isInteger(item.z)) errors.push(`${label}: invalid z`);
    if(pivotValid && boundsValid && (item.pivot[0] < 0 || item.pivot[1] < 0 ||
        item.pivot[0] >= item.bounds[2] || item.pivot[1] >= item.bounds[3])){
      errors.push(`${label}: pivot outside bounds`);
    }
    if(!Array.isArray(item.commands) || item.commands.length < 2){
      errors.push(`${label}: missing authored clusters`);
    }else if(boundsValid){
      for(const command of item.commands){
        const reason = commandError(command, item.bounds[2], item.bounds[3], paletteLength);
        if(reason) errors.push(`${label}: ${reason} command`);
      }
    }
    if(item.states === undefined) return;
    if(!item.states || typeof item.states !== "object" || Array.isArray(item.states)){
      errors.push(`${label}: invalid states`);
      return;
    }
    for(const [state, commands] of Object.entries(item.states)){
      if(!["loaded", "searching", "turned"].includes(state)) errors.push(`${label}: invalid state ${state}`);
      if(!Array.isArray(commands) || commands.length < 2){
        errors.push(`${label}.${state}: missing commands`);
      }else if(boundsValid){
        for(const command of commands){
          const reason = commandError(command, item.bounds[2], item.bounds[3], paletteLength);
          if(reason) errors.push(`${label}.${state}: ${reason} command`);
        }
      }
    }
  }

  function sameKeys(actual, expected){
    if(!actual || typeof actual !== "object" || Array.isArray(actual)) return false;
    const keys = Object.keys(actual).sort();
    return keys.length === expected.length && expected.every((key, index) => keys[index] === key);
  }

  function validateManifest(fixture){
    const errors = [];
    const data = arguments.length === 0 ? {BODY_IDS, PARTS, PROPS, MASKS, PALETTE,
      THEMES, PALETTE_ROLES, CAMERAS} : fixture;
    if(!data || typeof data !== "object" || Array.isArray(data)) return ["manifest: invalid fixture"];

    const bodyIds = Array.isArray(data.BODY_IDS) ? data.BODY_IDS : [];
    const parts = Array.isArray(data.PARTS) ? data.PARTS : [];
    const props = data.PROPS && typeof data.PROPS === "object" && !Array.isArray(data.PROPS) ? data.PROPS : {};
    const masks = data.MASKS && typeof data.MASKS === "object" && !Array.isArray(data.MASKS) ? data.MASKS : {};
    const palette = Array.isArray(data.PALETTE) ? data.PALETTE : [];
    if(!Array.isArray(data.BODY_IDS)) errors.push("body ids: invalid list");
    if(!Array.isArray(data.PARTS)) errors.push("parts: invalid list");
    if(bodyIds.length !== BODY_IDS.length || bodyIds.some((id, index) => id !== BODY_IDS[index])) errors.push("body ids: order mismatch");
    if(parts.length !== BODY_IDS.length) errors.push("body count differs from BODY_IDS");
    const ids = new Set();
    for(const part of parts) if(part && typeof part.id === "string") ids.add(part.id);
    if(ids.size !== parts.length) errors.push("body ids are not unique");
    for(let index = 0; index < BODY_IDS.length; index += 1){
      if(!parts[index] || parts[index].id !== BODY_IDS[index]) errors.push(`body id mismatch at ${index}`);
    }

    if(palette.length !== PALETTE.length || palette.some(color => typeof color !== "string" || !/^#[0-9a-f]{6}$/i.test(color))) errors.push("palette: invalid colors");
    const parents = new Set(["world", ...ids]);
    for(const part of parts) validatePiece(part, parents, errors, "part", palette.length, true);
    if(!sameKeys(props, [...PROP_NAMES].sort())) errors.push("prop keys: exact input contract required");
    for(const [name, propItem] of Object.entries(props)){
      if(!propItem || propItem.id !== name) errors.push(`prop key ${name}: item.id mismatch`);
      validatePiece(propItem, parents, errors, `prop:${name}`, palette.length, false);
    }

    if(!sameKeys(masks, [...MASK_NAMES].sort())) errors.push("mask keys: contact-belly and contact masks required");
    for(const [name, maskItem] of Object.entries(masks)){
      if(!maskItem || typeof maskItem !== "object"){ errors.push(`mask.${name}: invalid mask`); continue; }
      if(maskItem.id !== name) errors.push(`mask key ${name}: item.id mismatch`);
      const boundsValid = Array.isArray(maskItem.bounds) && maskItem.bounds.length === 4 &&
        maskItem.bounds.every(Number.isInteger) && maskItem.bounds[2] > 0 && maskItem.bounds[3] > 0;
      if(!boundsValid) errors.push(`mask.${name}: invalid bounds`);
      if(boundsValid && (maskItem.bounds[0] < 0 || maskItem.bounds[1] < 0 ||
          maskItem.bounds[0] + maskItem.bounds[2] > WORLD.width ||
          maskItem.bounds[1] + maskItem.bounds[3] > WORLD.height)){
        errors.push(`mask.${name}: world bounds exceeded`);
      }
      if(!Array.isArray(maskItem.commands) || !maskItem.commands.length){
        errors.push(`mask.${name}: invalid commands`);
      }else if(boundsValid){
        for(const command of maskItem.commands){
          const reason = commandError(command, maskItem.bounds[2], maskItem.bounds[3], palette.length);
          if(reason) errors.push(`mask.${name}: ${reason} command`);
        }
      }
    }

    const actualStates = [];
    for(const part of parts){
      if(!part || !part.states || typeof part.states !== "object" || Array.isArray(part.states)) continue;
      for(const state of Object.keys(part.states)) actualStates.push(`${part.id}@${state}`);
    }
    if(actualStates.length !== REQUIRED_STATE_KEYS.length || actualStates.some((key, index) => key !== REQUIRED_STATE_KEYS[index])) errors.push("state coverage: required state matrix mismatch");

    const byId = new Map(parts.filter(part => part && typeof part.id === "string").map(part => [part.id, part]));
    for(const part of byId.values()){
      const seen = new Set();
      let cursor = part;
      while(cursor && cursor.parent !== "world"){
        if(seen.has(cursor.id)){ errors.push(`parent cycle at ${part.id}`); break; }
        seen.add(cursor.id);
        cursor = byId.get(cursor.parent);
      }
    }

    const roles = data.PALETTE_ROLES;
    const themes = data.THEMES;
    if(!roles || !Array.isArray(roles.stableIndices) || !Array.isArray(roles.variableIndices) ||
       !Array.isArray(roles.roleByIndex) || roles.roleByIndex.length !== palette.length){
      errors.push("palette roles: invalid mapping");
    }else{
      const roleIndicesValid = [...roles.stableIndices, ...roles.variableIndices].every(index =>
        Number.isFinite(index) && Number.isInteger(index) && index >= 0 && index < palette.length);
      if(!roleIndicesValid){
        errors.push("palette roles: indices must be finite in-range integers");
      }else{
        const roleIndices = [...roles.stableIndices, ...roles.variableIndices].sort((a,b) => a - b);
        if(roleIndices.length !== palette.length || roleIndices.some((index, position) => index !== position)) errors.push("palette roles: invalid partition");
      }
      if(roles.stableIndices.some((index, position) => index !== PALETTE_ROLES.stableIndices[position]) ||
         roles.variableIndices.some((index, position) => index !== PALETTE_ROLES.variableIndices[position]) ||
         roles.roleByIndex.some((role, index) => role !== PALETTE_ROLES.roleByIndex[index])){
        errors.push("palette roles: authored mapping changed");
      }
    }
    if(!themes || !Array.isArray(themes.dark) || !Array.isArray(themes.light) ||
       themes.dark.length !== palette.length || themes.light.length !== palette.length){
      errors.push("theme: invalid palettes");
    }else{
      const validThemeColors = [...themes.dark, ...themes.light].every(color =>
        typeof color === "string" && /^#[0-9a-f]{6}$/i.test(color));
      if(!validThemeColors) errors.push("theme color: expected #rrggbb");
      if(themes.dark.some((color, index) => color !== palette[index])) errors.push("theme dark palette differs from PALETTE");
      const roleIndicesValid = roles && Array.isArray(roles.stableIndices) &&
        Array.isArray(roles.variableIndices) && [...roles.stableIndices, ...roles.variableIndices].every(index =>
          Number.isFinite(index) && Number.isInteger(index) && index >= 0 && index < palette.length);
      if(roleIndicesValid){
        for(const index of roles.stableIndices) if(themes.dark[index] !== themes.light[index]) errors.push(`theme stable index ${index} changed`);
        for(const index of roles.variableIndices) if(themes.dark[index] === themes.light[index]) errors.push(`theme variable index ${index} unchanged`);
      }
    }

    const cameras = data.CAMERAS;
    if(!cameras || typeof cameras !== "object" || !cameras.full || !cameras.compact){
      errors.push("camera: missing authored cameras");
    }else{
      for(const [name, camera] of Object.entries(cameras)){
        if(!camera || ![camera.x,camera.y,camera.width,camera.height].every(Number.isInteger) ||
           camera.width < 1 || camera.height < 1 || camera.x < 0 || camera.y < 0 ||
           camera.x + camera.width > WORLD.width || camera.y + camera.height > WORLD.height){
          errors.push(`camera.${name}: invalid crop`);
        }
      }
      if(cameras.compact.width > 180 || cameras.compact.height > 148) errors.push("camera.compact: crop exceeds 180x148");
    }
    return errors;
  }

  function themedPalette(theme){
    return theme === "light" ? THEMES.light : THEMES.dark;
  }

  function rasterTriangle(command){
    if(!Array.isArray(command) || command.length !== COMMAND_LENGTHS.poly ||
       command[0] !== "poly" || !command.slice(1).every(Number.isInteger)){
      throw new TypeError("rasterTriangle requires an integer poly command");
    }
    if(command[1] < 0 || command[1] >= PALETTE.length){
      throw new RangeError("rasterTriangle palette index is out of range");
    }
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for(let index = 2; index < command.length; index += 2){
      const x = command[index];
      const y = command[index + 1];
      if(Math.abs(x) > RASTER_LIMIT || Math.abs(y) > RASTER_LIMIT){
        throw new RangeError(`rasterTriangle coordinates exceed atlas-safe limit ${RASTER_LIMIT}`);
      }
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }
    if(maxX - minX > RASTER_LIMIT || maxY - minY > RASTER_LIMIT){
      throw new RangeError(`rasterTriangle coordinates exceed atlas-safe limit ${RASTER_LIMIT}`);
    }
    const points = [[command[2],command[3]], [command[4],command[5]],
      [command[6],command[7]]];
    const runs = [];
    for(let y = minY; y < maxY; y += 1){
      const scanY = y + 0.5;
      const intersections = [];
      for(let edge = 0; edge < 3; edge += 1){
        const a = points[edge];
        const b = points[(edge + 1) % 3];
        if(a[1] !== b[1] && scanY >= Math.min(a[1], b[1]) && scanY < Math.max(a[1], b[1])){
          intersections.push(a[0] + (scanY - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
        }
      }
      intersections.sort((a,b) => a - b);
      if(intersections.length >= 2){
        const start = Math.ceil(intersections[0] - 0.5);
        const end = Math.ceil(intersections[intersections.length - 1] - 0.5) - 1;
        if(end >= start) runs.push(Object.freeze([start, y, end - start + 1]));
      }
    }
    return Object.freeze(runs);
  }

  function drawTriangle(ctx, command, offsetX, offsetY){
    for(const run of rasterTriangle(command)){
      ctx.fillRect(offsetX + run[0], offsetY + run[1], run[2], 1);
    }
  }

  function drawCommands(ctx, commands, offsetX, offsetY, palette){
    for(const command of commands){
      ctx.fillStyle = palette[command[1]];
      if(command[0] === "px") ctx.fillRect(offsetX + command[2], offsetY + command[3], 1, 1);
      else if(command[0] === "run") ctx.fillRect(offsetX + command[2], offsetY + command[3], command[4], 1);
      else if(command[0] === "rect") ctx.fillRect(offsetX + command[2], offsetY + command[3], command[4], command[5]);
      else drawTriangle(ctx, command, offsetX, offsetY);
    }
  }

  function buildAtlas(canvasFactory, theme){
    if(typeof canvasFactory !== "function") throw new TypeError("canvasFactory must be a function");
    const canvas = canvasFactory(1024, 1024);
    const ctx = canvas.getContext("2d", {alpha:true});
    if(!ctx) throw new Error("PerezOS atlas requires a 2d canvas context");
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, 1024, 1024);
    const palette = themedPalette(theme);
    const rects = Object.create(null);
    let x = 0, y = 0, rowHeight = 0;
    function pack(key, item, commands){
      const width = item.bounds[2];
      const height = item.bounds[3];
      if(x + width > 1024){
        x = 0;
        y += rowHeight + 2;
        rowHeight = 0;
      }
      if(y + height > 1024) throw new Error("PerezOS atlas exceeds 1024x1024");
      drawCommands(ctx, commands, x, y, palette);
      rects[key] = Object.freeze({x, y, width, height,
        pivotX:item.pivot[0], pivotY:item.pivot[1]});
      x += width + 2;
      rowHeight = Math.max(rowHeight, height);
    }
    for(const part of PARTS) pack(part.id, part, part.commands);
    for(const part of PARTS){
      for(const [state, commands] of Object.entries(part.states)){
        pack(`${part.id}@${state}`, part, commands);
      }
    }
    for(const [name, propItem] of Object.entries(PROPS)){
      pack(`prop:${name}`, propItem, propItem.commands);
    }
    return Object.freeze({canvas, rects:Object.freeze(rects), keys:ATLAS_KEYS, palette});
  }

  const manifestErrors = validateManifest();
  if(manifestErrors.length) throw new Error(`Invalid PerezOS art manifest: ${manifestErrors.join("; ")}`);

  NS.Art = Object.freeze({WORLD, BODY_IDS, CAMERAS, PALETTE, THEMES, PALETTE_ROLES,
    ATLAS_KEYS, PARTS, PROPS, MASKS,
    RASTER_LIMIT, buildAtlas, compactCamera, validateManifest, rasterTriangle});
})(typeof window !== "undefined" ? window : globalThis);
