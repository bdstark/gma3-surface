# KB-24 hardware record: NX-K through `run` on macOS, 2026-10-11

The physical NX-K against onPC 2.5.1, with the operator at the keypad and the console read back over the MCP bridge.
First pass of the hardware qualification that KB-19 … KB-23 deferred.

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Apple silicon, `bdsmbpm401`), onPC 2.5.1.0, show `mcp-test-disposable`, user Admin, US layout, profile `Default` |
| Device | NX-K `11be:e102`, no serial, bus-powered |
| Console | `mtpnxk_surface` 0.11.0 (Plugin 2, `input=keyboard control=console force`) and the bridge 0.18.0 (Plugin 1) from main after GrandMA3MCP #28 and mtpnxk #9; modules hardkeys 0.10.0, feedback 0.7.0, control 0.7.0 |
| Service | release build of main (`1f15d2e`), `mtpnxk --verbose run`; the removal test re-ran on `fix/kb24-hardware-findings` (the logging fix below) |

| # | Check | Observed | Verdict |
| --- | --- | --- | --- |
| 1 | Link LED while paired | steady off (fresh feedback) | PASS |
| 2 | Keys and keyword LEDs: Clear, Cue 5, Clear, Record, Clear, HighLight twice | command line `Cue 5`, Cue LED on; cleared; `Store` with the Record LED on; HighLight LED blinking while on. Plugin: 8 presses, 8 releases, nothing refused, superseded or held | PASS |
| 3 | Rotaries 1–3 on encoder bank 4.1, fixtures 401 + 402 (R, G, B, Amber available) | the encoder bar followed every rotary; 487 motion events, 435 applied, 46 merged before resolution, 0 lost, duplicate or reordered, nothing left queued | PASS |
| 4 | Fine (Bank held) | Red ended at 19.2 %: the `.2` is the tenth-click Fine steps | PASS |
| 5 | Rotary push | refused `unsupported` (3 refusals), nothing changed | PASS (as specified) |
| 6 | Encoder feel | the NX-K encoders are **continuous, without detents**: one "detent" in the service and the console calibration is one report unit of the device. The operator found the speed right | PASS; docs corrected |
| 7 | Unplug with Record held | `nxk: read failed … device gone?`, `NX-K disconnected … its held keys are released`, `lost with 1 key(s) held (Record): released` within the 2 s retry; the console held nothing afterwards (hardkeys capacity 0) | PASS |
| 8 | Replug | `NX-K open` within the retry; LEDs repainted (Record lit: `Store` was still pending, a release never erases text) | PASS |
| 9 | Plugin stop and start | Link LED blinked within ~1.5 s of `Plugin 2 "stop"` and went steady when the service re-paired | PASS |
| 10 | Holding Record | `Store` typed at key-down, the store options opened at ~2 s. onPC's own on-screen Store key (Command Section, F3) does exactly the same with a mouse hold, so the surface matches the console | PASS (no change) |

## Findings

- **Inconsistent device logging (fixed on `fix/kb24-hardware-findings`).** The three devices shared one "absence
  announced" flag that only the NX-K set: while the NX-K was attached the M-Play's failure line repeated every 2 s,
  and the M-Touch never reported a failed open. The repeating line buried the unplug messages in the first removal
  run, which looked like an undetected unplug. Each device now has its own flag and the same wording (`<device>: not
  attached (…); retrying every 2 s` once per absence, `<device>: … open; … live` on open); unit test
  `each_device_announces_its_absence_once_per_absence`.
- **A stray packet at unplug.** As the cable came out with Record held, the NX-K delivered
  `01 02 01 51 01 01 02 01 54 01` (two five-byte key reports, Edit and Record down, the M-Touch's report format). The
  NX-K decoder does not recognise it, logs it and drops it; nothing reached the console. Recorded, not acted on.
- **A brief M-Touch attach during the run** opened, bound its 30 executors, was unplugged (`0xe00002ed`), ended its
  gestures and unbound its window cleanly. Not a structured M-Touch test.
- **Plugin counters:** `presses` counts holds started, `releases` only explicit key-ups that ended a hold; holds ended
  by a session closing or by the hardkeys module do not move `releases` (47 against 46 here with nothing held).

Cleanup: `ClearAll` after the rotary test; the command line's `Store` was left for the replug check.

Not tested here: the M-Touch and M-Play through `run` (parameter strips, playback, bars and keys), two surfaces at
once, Windows and Linux hosts.
