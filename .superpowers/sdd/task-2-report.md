# Task 2 Report: Authored Indexed Pixel Art

## RED / GREEN / REFACTOR

- RED: added `tests/perezos/test_art.js`, then ran
  `node --test tests/perezos/test_art.js`. The run failed with
  `MODULE_NOT_FOUND` for the not-yet-created `dash/perezos/art.js`, which
  confirmed the new public art contract was absent.
- GREEN: created `dash/perezos/art.js`; the focused run passed all 7 tests.
- REFACTOR: kept manifest construction, freezing, validation, scanline polygon
  filling, and atlas packing in small initialization-only helpers. The focused
  suite remained green after review.
- REVIEW RED: added failing assertions for the exact 98-key atlas, complete
  state rasters, compact crop semantics, asymmetric anatomy, selective theme
  roles, malformed validation fixtures, and four triangle coverage goldens.
  Each failed against the reviewed implementation for the intended reason.
- REVIEW GREEN: implemented each reviewed contract in sequence. The expanded
  focused suite passes all 13 tests, including ten anatomical family goldens.
- SECOND REVIEW RED: added failing audits for variable-index use and actual
  themed body pixels, hostile validation fixtures, and malformed public
  triangle commands. The failures reproduced the remaining unsafe palette use,
  validation gaps, and unguarded API seam.
- SECOND REVIEW GREEN: moved body specular index 20 into the stable identity
  partition, replaced the nose's cable/metal index, made validation total for
  the additional fixtures, and gave `rasterTriangle` strict error behavior. The
  focused suite now passes all 15 tests.
- DEFENSIVE RED / GREEN: a public triangle with a huge integer span could enter
  an unbounded scanline loop. A failing boundary test established the 1024-unit
  contract; `rasterTriangle` now rejects unsafe coordinates and spans before
  points or scanline arrays are allocated. The focused suite passes 16 tests.

## Public interfaces

`ComandOSPerezOS.Art` exports the frozen `WORLD`, `BODY_IDS`, `CAMERAS`,
`PALETTE`, `THEMES`, `PALETTE_ROLES`, `ATLAS_KEYS`, `PARTS`, `PROPS`, and
`MASKS` data plus `buildAtlas(canvasFactory, theme)`,
`compactCamera(stageWidth, stageHeight)`, `validateManifest(optionalFixture)`,
the public `RASTER_LIMIT`, and the deterministic `rasterTriangle(command)`
scan-conversion primitive. It consumes the existing `ComandOSPerezOS.Core`
namespace without modifying it.
`rasterTriangle` remains deliberately public because it is a reusable crisp
geometry primitive rather than a test-only atlas hook; malformed shape/type
commands throw a stable `TypeError`, and out-of-range palette indices throw a
stable `RangeError`. Every coordinate must be within ±`RASTER_LIMIT` (1024),
and each axis span must be at most 1024. Violations throw a stable `RangeError`
before variable-size raster allocation.

`buildAtlas` requests one 1024 by 1024 canvas, disables image smoothing, clears
it once, and emits frozen deterministic rectangles for base parts, complete
state sprites, and props. Rasterization uses only integer-coordinate `fillRect`
calls, including a pixel-center, half-open integer scanline fill for authored
triangle commands. Masks remain frozen logical contact regions for dynamic
interaction checks and are intentionally not atlas sprites.

## Review fixes

- Atlas completeness: stable keys are base `id`, state `id@state`, and prop
  `prop:name`. The single atlas contains 48 base sprites, 42 complete state
  sprites, and 8 props. All 98 rects are frozen, deterministic, non-overlapping,
  bounded, and contain raster output.
- Compact camera: stages that fit the world receive the full composition at an
  integer scale. Smaller stages receive a centered sub-crop of the authored
  180 by 148 compact camera at 1x, with integer source/destination coordinates
  and no stage overflow.
- State silhouettes: replacement sprites provide searching/turned head shapes,
  turned/foreshortened far-side arm and leg chains, and loaded contact-sensitive
  variants for all 12 claws. Remaining state sprites are materialized as
  complete base-plus-authored-overlay command sets at manifest construction.
- Anatomy: near/far arms and legs now have distinct polygonal contours,
  asymmetric bends, highlight runs, and dither pixels. Rear/front and left/right
  claw families use materially different hook profiles. Golden raster hashes
  cover head, torso, four limb chains, and four claw families.
- Theme roles: warm identity indices 1 through 11 are byte-identical in both
  themes. Only declared deep-shadow, state-light, cable/metal, and small-prop
  sensitive indices vary.
- Validation: zero-argument validation still checks the public manifest;
  optional malformed fixtures now always return errors rather than throwing.
  Checks cover exact IDs/props/masks/state coverage, safe geometry and commands,
  palettes/themes/roles, cameras, parents, and cycles.
- Polygon coverage: flat-top, flat-bottom, thin, and slanted triangles share one
  pixel-center, half-open edge rule with exact golden runs.

## Second review follow-up

- Palette usage is now audited from actual commands, not inferred from role
  declarations. Base body art may vary only deep-shadow index 0; complete state
  sprites may additionally use their declared loaded/searching/turned lights;
  cable, metal, fire, and cloth indices remain prop-sensitive. Nose metal was
  replaced with stable bone, and specular index 20 is stable in both themes.
- A light/dark raster audit checks every base-body occupied pixel and proves all
  non-shadow identity pixels—including jaw, nose, eyes, and claws—are identical.
- Fixture validation now rejects body and mask bounds outside the 224 by 192
  world while allowing intentionally extending props such as `corona`.
  Prop/mask keys must equal `item.id`, every theme entry must be `#rrggbb`, and
  role indices are checked as finite in-range integers before sorting or array
  access. Symbol, BigInt, NaN, and string fixtures return errors without throws.

## Authored data counts

- World: 224 by 192 logical pixels.
- Body: exactly 48 stable IDs, including 12 independently named claws.
- Indexed palette: 21 colors, including a seven-step warm-brown identity ramp,
  bone/mouth/metal accents, and separate loaded/searching/turned state lights.
- State artwork: explicit `loaded`, `searching`, and `turned` command sets on
  the pieces that visually express each state; 42 complete cached variants.
- Props: 8 (`corona`, `casco`, `visor`, `fuego`, `hamster`, `gordo`, `huevo`,
  and `bufanda`).
- Contact masks: 4, including `contact-belly`.
- Camera presets: 4, including the authored compact crop.
- Atlas: 98 stable cached sprites in one 1024 by 1024 canvas.
- Raw `dash/perezos/art.js` source size: 37,097 bytes.

## Verification

- `node --test tests/perezos/test_art.js`: 16 passed, 0 failed.
- `node --test tests/perezos/*.js`: 21 passed, 0 failed.
- `pytest -q tests/test_perezos.py`: 1 passed.
- `node --check dash/perezos/core.js` and
  `node --check dash/perezos/art.js`: passed.
- `git diff --check`: passed.
- `wc -c dash/perezos/art.js`: 37,097 bytes.
- Full `pytest -q` was not rerun for the final defensive branch: it changes no
  authored command, palette, manifest, camera, or atlas output, and all exact
  raster goldens plus all PerezOS Node/Python tests passed. The preceding full
  post-authored-change run remains 385 passed in 93.89 seconds.

## Self-review

- Anatomy: the ordered ID list exactly matches the brief. Every piece has an
  explicit parent, local pivot, world bounds, numeric z group, and at least two
  indexed authored clusters. Limbs connect through upper/lower/wrist-or-ankle/
  palm chains, and all claws attach to their corresponding palm. Near/far limbs
  no longer share template occupancy, and exact family goldens protect the
  authored head, torso, limb, and claw silhouettes.
- Data validity: total validation checks ID order/uniqueness, exact prop/mask and
  state contracts, parent links/cycles, safe pivots and bounds, z groups,
  command arity, integer geometry, palette ranges, local command bounds,
  themes/roles, and cameras. All exported manifest records and atlas rectangles
  are frozen.
- Determinism: atlas order is the stable body order, packing has a fixed
  two-pixel gutter, theme selection preserves indices, and repeated builds
  produce identical rectangle metadata and raster calls.
- Theme safety: command-level and raster-level audits prevent self-declared role
  metadata from hiding identity-color changes. Only deep body shadows and
  explicitly state/prop-sensitive pixels differ between themes.
- Rendering constraints: artwork consists of authored `px`, `run`, `rect`, and
  `poly` clusters. There are no paths, ellipses, text, filters, shadows,
  smoothing, or fractional-alpha operations.
- Hot-path allocations: manifest data and complete state command arrays are
  constructed and frozen once; palette selection reuses frozen arrays. Canvas,
  rectangle records, and polygon scanline scratch data are allocated only
  during atlas construction, not during frame rendering. All frame consumers
  can resolve frozen rects by stable key. Public triangle inputs are bounded
  before points, intersections, or output-run arrays can scale with their span.
- Scope: only the Task 2 art module, its focused tests, and this report were
  added. Task 3 rendering/rig behavior was not started, and Task 1/core files
  were not changed.
- Known concerns: none within the Task 2 contract.
