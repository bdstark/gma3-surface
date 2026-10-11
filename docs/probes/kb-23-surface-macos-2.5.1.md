# KB-23 surface live record: the lights rendered from the console context (macOS, onPC 2.5.1.0)

Run on 2026-10-10 (local time) against show `mcp-test-disposable` on onPC 2.5.1.0 with `mtpnxk_surface` **0.9.0**
(Plugin 2, `input=fake control=console force`, vendored feedback 0.6.0 sha256 `1126d6f3…` from GrandMA3MCP `45945bf`)
next to the bridge 0.18.0 (Plugin 1, used for the console commands and the read-backs). The service was the release
build of this branch, driven with `mtpnxk --plugin 127.0.0.1:9810 --key-file … sim --script …` (the simulated M-Touch:
four parameter strips on slots 1-4, playback rows `101,201,301` at bank 1). The `out MTouch …` lines are the device
writes the loop would have sent (decoded bars: lane + lit LED count, `*` = blinking); the `light:` lines are the
textual state printed at the end of each run. No physical M-Touch was connected (KB-24).

Setup (through the bridge, undone at the end): `Assign Sequence 5527 At Page 1.101` (LOS2 All DelayA, appearance
Orange `FF7F00FF`, idle), `Assign Sequence 5511 At Page 1.102` (LOS1 Odd, no appearance), `Assign Sequence 5509 At
Page 1.209` (LOS1 All DelayA, Orange, **running**, Master 100), `Assign Sequence 5528 At Page 1.309` (LOS2 All DelayB,
Orange, idle); the fresh assignments came up Go+/Master. Executors 201-208 (StartShow, Master 0) and 210 (Rotate,
Temp) were already on the page and were not pressed.

| Run | Script | Device writes (decoded) | `light:` lines | Verdict |
| --- | --- | --- | --- | --- |
| A rest | `Bank:tap@3500`, 4.5 s | `led 4201 <- 0100` (PFA1 blue), `led 4211 <- 0100`, `led 4282 <- 0100` (PFB9), `led 4205/4215/…/4295 <- 0002` (PFD red: assigned, idle), **`bar 4283 <- G10 R10`** (PB9: ten LEDs, yellow lanes), **`led 4285 <- 0001`** (PFD9 green: running); every empty executor and every level-0 bar dark | `PFA1 ex101: Sequence "LOS2 All DelayA" Go+/Master level=100% idle colour=Yellow (Definition) -> Assigned`, `PB9 ex209: … level=100% running colour=Yellow (Definition) -> Running`, `PB1 ex201: Sequence "StartShow 1" Flash/Master level=0% idle colour=Green (Neutral) -> Assigned`, `PFA2 ex102: … colour=Green (Neutral)`, 17 x `empty, dark`; `strip1 slot1: Dimmer "Dim" Dimmer/Dimmer colour=White (Category…) avail=no-selection value=none` | PASS |
| B1 single value | `Fixture 401`, `Select EncoderBank 4.1`, `Attribute "ColorRGB_R" At 50`; `Bank:tap@3500` | `led 6111/6121/6131/6141 <- 0100` (keys: adjustable), **`bar 6112 <- B5`**, `bar 6122 <- B10`, `bar 6132 <- B10` (G and B at 100 after the group activation), bar 6142 dark (Amber 0) | `strip1 slot1: ColorRGB_R "R" Color/RGB [ColorRGB] colour=Red (Definition; the strip's bar is blue-only) avail=available value=50% single (5/10)`, strip2 `ColorRGB_G … colour=Green (Definition) … 100% single (10/10)`, strip3 Blue, strip4 `ColorRGB_RY "Amber" … colour=Yellow (Definition) … 0% single (0/10)` | PASS |
| B2 mixed + touch | `Fixture 402`, `Attribute "ColorRGB_R" At 20`, `Fixture 401 + 402`; `strip1:t100@3000,strip1:lift@600` | **`bar 6112 <- B0b1000000001*`** (the two end LEDs, blinking), `led 6111 <- 0500` while touched (blue fast), `<- 0100` after the lift; the touch travelled as a hold on slot 1 (`control: events=2 sent=2 … refused=0`) | `strip1 slot1: ColorRGB_R … value=mixed (end LEDs blinking)` | PASS |
| C pressed, running | `pfa1:down@3000,pfa1:up@1500`, 6.5 s | `led 4201 <- 0100` -> **`0500`** (held: blue fast) -> **`1100`** (released, the sequence runs: blue slow); the plugin ran `Press Page 1.101`, `Unpress Page 1.101` (its command log), Sequence 5527 active afterwards (ended with `Off Sequence 5527`) | `PFA1 ex101: … level=100% running colour=Yellow (Definition) -> Running` | PASS |
| D stale | `Bank:tap@7500`, 8 s; `Plugin 2 "stop"` at 3.5 s, restarted after the run | before: `led 4201 <- 0100`, `bar 4283 <- G10 R10`, `led 4285 <- 0001`, `led 4295 <- 0002`, `led 6111 <- 0100`, `bar 6112 <- B0b1000000001*`; after the stop: **`bar 4283 <- R10*`** (all ten red, blinking), **`led 4285 <- 0042`**, `led 4295 <- 0042` (red slow), **`led 4201 <- 1100`**, `led 6111 <- 1100` (blue slow), **`bar 6112 <- B10*`**; `link: paired=false … link_downs=1` | `strip1 slot1: unavailable (no fresh context), shown as unavailable`, `PFA1 ex101: unavailable …, shown as unavailable`, `PB9 ex209`, `PFB9 ex309`, `PB1 ex201` likewise; the dark empties stayed dark | PASS |

Counters of run D: `lights: enabled=true renders=5556 flushes=4 writes=101 coalesced=0 stale_renders=2058 lit=31`.

## Also observed

- `Attribute "ColorRGB_R" At 50` on fixture 401 put G and B at 100 into the programmer as well (the colour group's
  activation, KB-17); the strips showed exactly that: it is console state, not an artefact.
- Right after a pairing the first context can still carry a level or activity the console changed in the last second
  (run D saw PFA1 as running once before the `Off` took effect in the plugin's 1 s refresh): the renderer repainted on
  the next context; nothing was invented.
- The decoded `out` echo (`G10 R10`, `B0b1000000001*`) made the bar payloads readable in the sim output; the hardware
  rendering of the yellow mix and the slow blink remains KB-24's.

## Cleanup

`Off Sequence 5527`, `ClearAll`, `ClearSelection`, `Select EncoderBank 1.1`, `Delete Page 1.101`/`102`/`209`/`309`
(`/NoConfirmation`); read back: the four executors empty, bank 1.1, selection 0, programmer empty (70/70 fixtures
scanned), Sequence 5527 inactive, Sequence 5509 still running on 291 as before. Plugin 2 restarted with the same key.
The operator's 8-fixture selection that was active before the MCP probe is not restored (it was cleared for that run).
