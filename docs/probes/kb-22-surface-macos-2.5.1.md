# KB-22 live record: playback faders and buttons through the surface service (macOS, onPC 2.5.1.0)

Run on 2026-10-10 against show `mcp-test-disposable` on onPC 2.5.1.0: `mtpnxk_surface` **0.8.0** imported as Plugin 2
(vendored control 0.5.0 at the KB-22 pin `0315e18`, sha256 `c0716917…`; feedback 0.5.0; hardkeys 0.10.0) and started
`key=<fresh keygen> input=fake control=console force` next to the MCP bridge 0.18.0 (Plugin 1, `lua input=keyboard
control=console`, used for the console reads and the restores). Both plugins were (re)imported and started from a macro
written through the bridge's Lua (`Delete Plugin N /NoConfirmation`, `Import Plugin Library "…" At Plugin N`,
`ReloadAllPlugins`, the two start lines), nothing was typed on the console. The release service's `sim` drove the plugin
with `--pb-rows 201,201,301` (the fader row's executors on all three rows: PFA n / PB n / MPA n = executor 20n's key and
fader) on page 1; `--verbose`. Evidence: the service's `banks:` summary and log lines, and the plugin's own console-backend
command log and counters read through the bridge (`__mtpnxk_surface.modules.control.instance`), plus the sequences'
`HasActivePlayback()` / `GetFader` after each run.

Page 1 executors used: 201 (Sequence 2 "StartShow 1": Flash key, Master fader; its Master at 0 and the sequence off), 195
(Sequence 5531: Toggle key), 210 (Sequence 5437: Temp key, **Temp fader** at 0).

| Run | Script (`sim --script`) | Service summary | Plugin's console commands / console state | Verdict |
| --- | --- | --- | --- | --- |
| 1 buttons and the Go profile (`--pb-go 195`) | `pfa1:down@1500,pfa1:up@900,go:tap@600,go:tap@1200` | `downs=3 ups=3 dedicated=2`, 6 control events, 0 refused | `Press Page 1.201`, `Unpress Page 1.201`, `Press Page 1.195`, `Unpress Page 1.195`, `Press Page 1.195`, `Unpress Page 1.195`, all applied; Sequence 5531 (Toggle) back off after the two taps | PASS: the strip button and the Go key press the executor's configured function; nothing is translated |
| A fader below an unreachable level | `pb1:t100@1500,pb1:m200@400,pb1:m255@400,pb1:m128@400,pb1:lift@400` (201's Master at 0) | `touches=1 lifts=1 positions=0 pickups=0 waiting=3` | no command (two noops: the touch hold and its lift); level stays 0 | PASS: no position goes out while the strip never crossed the executor's level |
| C M-Play pressure button and fader (201 at 100 for the run) | `mpa1:down@1500,mpa1:up@900,mpf1:t255@500,mpf1:m250@300,mpf1:lift@300` | `downs=1 ups=1 touches=1 lifts=1 positions=1 pickups=1 waiting=0` | `Press Page 1.201`, `Unpress Page 1.201`, `FaderMaster Page 1.201 At 98.0392`; level 98.04 | PASS: the pressure report is one down and one up; the touch at the level picks up at once and the move is placed |
| D bank change while touched | `pb1:t255@1500,pb1:m250@300,pbank:up@500,pb1:m200@500,pb1:lift@400,pbank:down@300` | `changes=2 positions=1 pickups=1 rebound=1`; log "mtouch/PB1: the binding was replaced while touched; lift and touch again" | `FaderMaster Page 1.201 At 98.0392` before the bank change only; the lift's touch-up went to the frozen 201 (noop) | PASS: the bank change rebinds the touch, the later move is dropped, the lift ends the original hold |
| B external edit while held | `pb10:t0@1500,pb10:m5@300,pb10:m10@300,pb10:m12@3400,pb10:m15@300,pb10:m0@300,pb10:lift@300` with `FaderTemp Page 1.210 At 30` issued through the bridge 3.0 s in | `positions=2 pickups=1 waiting=3 resets=1`; log "mtouch/PB10: the executor's level moved to 30% while the strip was at 4% (edited elsewhere); pick it up again" | `FaderTemp Page 1.210 At 1.9608`, `At 3.9216`, then nothing; the Temp stayed at 30 (active) until `FaderTemp Page 1.210 At 0` ended it | PASS: a level edited elsewhere resets the pickup; the strip no longer drives the executor until it crosses the new level |
| A2 pickup across the level (210's Temp fader at 0) | `pb10:t50@2500,pb10:m20@400,pb10:m0@400,pb10:m100@400,pb10:m0@400,pb10:lift@400` | `touches=1 lifts=1 positions=3 pickups=1 waiting=1` | `FaderTemp Page 1.210 At 0`, `At 39.2157` (the Temp playback starts), `At 0` (it ends); 210 at 0 and inactive afterwards | PASS: touch above the level waits, crossing it picks up, the configured Temp function is placed over its travel |

Also observed:

- A first attempt at A2 was rebound at once ("the binding changed while touched") with `retransmitted=4` on the link: no
  context reached the service for more than `state_stale` (1.5 s), so the generation was unknown and the touch was ended
  as the design says (an unknown binding never carries a position). The repeat (above) ran clean. The plugin sends a
  context at least every second; a stall longer than that costs one touch.
- The pickup bar writes (`out MTouch bar 4203/4293 <- …`) were produced by the sim's output path; the physical bars were
  not observed (KB-24).
- `FaderMaster Page 1.201 At 100`, issued through the bridge to restore 201 between runs, **started** Sequence 2
  ("StartShow 1": its Master going from 0 above 0 auto-starts it); it was ended with `Off Sequence 2` and its Master put
  back to 0. The fader runs then used 210's Temp fader, whose level is meant to start and stop the playback. The
  service itself never raised a level the operator had not reached with the strip.
- No refusal, loss or unresolved record in any run; both plugins were left running (bridge Plugin 1 `control=console`,
  surface Plugin 2 `input=fake control=console`), the macros that started them deleted.

## Re-run after the PR #7 / #26 review round 1 (2026-10-10)

Plugin 0.8.0 with control 0.5.0 from `3e0e7c4` reinstalled and restarted the same way; the service rebuilt with the
revision gate and the new external-edit window:

| Run | Script | Result | Verdict |
| --- | --- | --- | --- |
| R6 press right after a bank change | `pbank:up@1500,pfa1:tap@250,pbank:down@600` | `link: bind acknowledged: binding revision 3`, then `event 3 refused [target-unavailable] executor 211 is empty`; `stale=0`, no command | PASS: the tap 250 ms after the bank change reached the plugin against the new binding (bank 2's 211, empty), never `stale-binding` |
| R5 two controls on one executor (`--pb-go 201`) | `pfa1:down@1500,go:down@400,go:up@400,pfa1:up@400` | `event 3 refused [conflict] execDefault#1/1.201.key|…|Flash is being operated by this session (button on mtouch/PFA1)`; plugin commands `Press Page 1.201`, `Unpress Page 1.201` only; Sequence 2 off afterwards | PASS: the alias never presses a held executor twice; one Press, one Unpress |

## Re-run after review round 2 (2026-10-10)

Control 0.5.0 from `ce91371` (the verification of a frozen release is the plugin's live `deps.observe` read only):

| Run | Script | Result | Verdict |
| --- | --- | --- | --- |
| R-live: a button held across a bank change | `pfa1:down@1500,pbank:up@500,pfa1:up@900,pbank:down@400` | `Press Page 1.201` applied; after `PLAYBACK BANK +` the bound window is 211-220 (201 no longer in the binding); the release was verified by the live read of page 1's executor 201 (same object, same pool) and `Unpress Page 1.201` applied; `frozen=1`, nothing unresolved, Sequence 2 off | PASS |

Not verified: the physical M-Touch/M-Play through `run` (their pressure reports and bars: KB-24), Rate/Speed/crossfade
functions (unqualified on the console half), an assignment changed while a fader was touched (the key-element case is in
the MCP record and the harness).
