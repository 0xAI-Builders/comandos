<!-- Production copy for CommandOS 1.0 Pomodoro: only the files listed in manifest.json
     were copied unchanged; each SHA-256 was verified against the design manifest.
     Quintino and the replaced 7Soul/karsiori clock sheets are not shipped. The original
     text below is kept verbatim, including paths that point to the design worktree. -->

# Pixel art credits and licenses

Downloaded on 2026-09-28 for the ComandOS design prototype. These PNGs are original files from the artist packs; no AI image generation or pixel repainting was used. CSS selects sprite cells, displays integer scaling, adds gentle movement to static artwork, and plays the original frame strips from La Red Games. The one-pixel transparent border in 7Soul icons is clipped for the preview. No source PNG was changed.

## Shikashi's Fantasy Icons Pack (free, v2)

Artist: **Matt Firth (shikashipx)**. Derivative design credit: **game-icons.net**.

- Source: https://shikashipx.itch.io/shikashis-fantasy-icons-pack
- Current artist page license: **Creative Commons Attribution 4.0 International**.
- License: https://creativecommons.org/licenses/by/4.0/
- Archive: Shikashi's Fantasy Icons Pack v2.zip (free download id 2140234).
- The bundled 2020 author notes still mention game-icons.net CC BY 3.0. They are retained verbatim in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs/shikashi/AUTHOR-NOTES.txt. The current artist page explicitly updates the pack to CC BY 4.0 and requests both credits above. Earlier reference: https://creativecommons.org/licenses/by/3.0/
- Original artwork and any indicated rights remain with the credited creators. No endorsement is implied.

## Gems / Coins Free

Artist: **La Red Games**.

- Source: https://laredgames.itch.io/gems-coins-free
- License: **CC0 1.0 Universal**, as stated in the page's asset-license metadata and confirmed by the artist.
- License: https://creativecommons.org/publicdomain/zero/1.0/
- Archive: Coin_Gems.zip (free download id 1021985).
- 16 × 16 native frames: five for coins, four for gems. The prototype plays these existing frames; it does not claim they are newly drawn.

## 496 pixel art icons for medieval/fantasy RPG

Artist: **Henrique Lazarini (7Soul1)**. Compilation submitted by **gnola14**.

- Distributed source: https://opengameart.org/content/496-pixel-art-icons-for-medievalfantasy-rpg
- Artist source: https://www.deviantart.com/7soul1/art/420-Pixel-Art-Icons-for-RPG-129892453
- Compilation label: **CC0**. The artist description says **Public Domain** and documents removal of derivatives of commercial game icons. DeviantArt's metadata also retains **CC BY 3.0**. Preserve attribution and both license references rather than silently asserting that those labels agree.
- CC0: https://creativecommons.org/publicdomain/zero/1.0/
- CC BY 3.0: https://creativecommons.org/licenses/by/3.0/
- Archive: https://opengameart.org/sites/default/files/496_RPG_icons.zip
- Only the six unchanged PNGs used in this prototype are included. The newer paid itch.io pack is a separate product and was neither purchased nor copied.

## Free Assorted Icons

Artist: **Quintino Pixels**.

- Source: https://quintino-pixels.itch.io/assorted-icons
- License: **CC0 1.0 Universal**, in the title and explicitly confirmed by the artist in the page's comments.
- License: https://creativecommons.org/publicdomain/zero/1.0/
- Archive: Icons.rar (free download id 1987450).
- Only the unchanged PNGs used by the prototype are included.

## Integrity

Per-file SHA-256 and size records are in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs/manifest.json. Source archive SHA-256:

- shikashi: `eb3725a63ec7afa04c8942057717758cc0a9388ceb1d4348a7e8487dfbc0492e`
- lared: `5903e06b9a7be1c7f4452fae18bed3403645b37b53d24e2ff08007df2c6718da`
- 7soul: `82167b463d20fdc2c0cb641e061503bb77816558087270e50eb66ad34cf75658`
- quintino: `88450f5eddf2380b36e8eac84e3abb18a053755f947ad6d456c8e1f0295ab39f`


## Alquimia with original frame animation

Artist: **karsiori**.

- Source and explicit CC0 license statement: https://karsiori.itch.io/pixel-art-potion-pack-animated
- License: https://creativecommons.org/publicdomain/zero/1.0/
- The free archive downloaded through the artist's itch.io page has upload id 9132289. Its SHA-256 and each original source-member name are recorded in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs/manifest.json.
- Six unchanged horizontal PNG sprite sheets are vendored. No pixels were painted, generated, recolored or resampled. CSS selects the original frames at the artist's suggested preview rate of 10 fps, with the frame window centered in a fixed icon slot.
- Clock role: purple Bubbly Brew Bottle Rising, 18×35 pixels, **22 actual frames** in the downloaded sheet. The artist page and older pack notes list seven for this item; the downloaded 396-pixel-wide sheet and 22 separate source sprites establish the count used here.
- XP: blue Small Elixir, 15×30, seven frames. First block: teal Small Vial, 14×24, nine frames. 100 minutes: purple Large Jar, 18×34, 24 frames. Consistency: gold/purple Encased Potion, 14×25, eight frames. Level: gold Glowing Potion, 24×39, 12 frames.
- The animated focus bottle accompanies the numeric timer; its decorative liquid does not indicate remaining time. The previous Quintino hourglass is not presented as a frame-animated asset.
- Earlier Quintino provenance remains above as the record for the previous static-art proposal.


## Cristales style

Artist: **karsiori**.

- Source and explicit CC0 license: https://karsiori.itch.io/free-pixel-art-gem-pack
- License: https://creativecommons.org/publicdomain/zero/1.0/
- Free archive upload id 9281968. Archive SHA-256, original archive member names, sizes and file hashes are in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs/manifest.json.
- Six unchanged horizontal sheets from GEM 1, 2, 3, 4, 5 and 9: purple, turquoise, light green, blue, red and gold. The actual sheet widths establish 10, 10, 11, 11, 11 and 10 frames respectively. Frame windows are 18×30, 23×27, 28×28, 20×30, 19×22 and 27×26 pixels.
- The prototype plays the artist's original images at 10 fps with fixed icon slots. No pixels were repainted, generated or resampled. The additional style does not replace the five earlier visual options.


## Expressive clock, chest and fire animations

The previous entries describe earlier design rounds. The common clock now uses an actual animated hourglass in all six styles and in the header; it replaces the static clocks, subtle potion/gem clock roles and slow line icon. Other potion and gem roles remain.

- **ComandOS** (own asset, 30-sep-2026), `comandos/hourglass.png`: 864×32 strip of 27 frames. The empty brass-and-glass hourglass was generated with gpt-image-2.5-flare; the sand, the stream and the flip were painted by the project's `hourglass.py` (see `dash/prototypes/assets/hourglass/comandos/README.md`). Frames 0-20 fill the hourglass (ComandOS maps them to the elapsed share of the Pomodoro block), frames 21-26 flip it (played once when a block ends). It replaces the Davitheoles hourglass used in the 29-sep round, which is no longer shipped.
- **karsiori**, FREE Pixel Art Chest Pack - Animated: https://karsiori.itch.io/pixel-art-chest-pack-animated — **CC0 1.0**, https://creativecommons.org/publicdomain/zero/1.0/. Free archive upload id 8554113. The unchanged Golden Chest 1 sheet has five distinct 40×25 frames (closed state plus four opening frames). The prototype plays those frames forward, holds the open position, and closes by reversing them in a three-second loop. This sequencing is ours; the original pixels are unchanged. Used for the 100-minute reward in Arcade, Fantasía and RPG clásico.
- **ArlanTR**, Campfire pixel art animated: https://opengameart.org/content/campfire-pixel-art-animated — **CC0 1.0**, https://creativecommons.org/publicdomain/zero/1.0/. Original source: https://opengameart.org/sites/default/files/campfire-sprite-sheet.png. Unchanged 128×32 PNG with four distinct 32×32 frames, played at 8 fps. Used for consistency in Fantasía and RPG clásico.

All three animations appear together in the review gallery, with artist links. The hourglass loops decoratively in idle, running, paused and completed states, as requested for visible icons; sand level does not encode remaining minutes. The numeric timer and state label remain authoritative. System reduced-motion preferences stop the animations. No paid assets were purchased or copied. Hashes and original source identifiers are recorded in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs/manifest.json.
