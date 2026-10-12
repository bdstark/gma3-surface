# KB-24 hardware record: M-Play through `run` on macOS, 2026-10-12

The physical M-Play next to the NX-K against onPC 2.5.1, the operator at the surface, the console read back over the
MCP bridge. Third part of the hardware pass ([NX-K](kb-24-hardware-nxk-macos.md),
[M-Touch](kb-24-hardware-mtouch-macos.md)).

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Apple silicon, `bdsmbpm401`), onPC 2.5.1.0, show `mcp-test-disposable`, user Admin |
| Devices | M-Play `11be:f80c` and NX-K `11be:e102` (the M-Touch not attached) |
| Console | the show had been reloaded without the surface plugin: `mtpnxk_surface` 0.11.0 reinstalled as Plugin 2 from main (`dce8554`) with modules hardkeys 0.10.0, feedback 0.8.0, control 0.7.0 (Macro 150 recreated for it); bridge 0.18.0 as Plugin 1 |
| Service | release build of main (`dce8554`), `mtpnxk --verbose run` in a Terminal panel tab |
| Bound | 60 executors: the left section (12 wide) rows 101-112 / 201-212 / 301-312, the right section (24 wide) 401-424; page 1 holds 201-208 StartShow 1-8 (Flash/Master), 210 Rotate (Temp), 401 Beat and 402 Position Speed (speed masters, key LearnSpeed); a temporary `Assign Sequence 2 At Page 1.101` with key Flash (verified empty first, removed afterwards) |

| # | Check | Observed | Verdict |
| --- | --- | --- | --- |
| P1 | top button 1 (MPA1, pressure key) on 101, Flash | StartShow 1 ran while held and stopped at the release (`HasActivePlayback`), each time | PASS |
| P2 | fader 1 (MPF1) on 201 | positions applied as `FaderMaster` on 201 after pickup; back at 0 at the end, the sequence not running | PASS |
| P3 | right-section button 1 (MPE1) on 401, LearnSpeed | refused `unsupported`: "the configured key function LearnSpeed is not qualified yet"; nothing changed | PASS (as specified) |
| P4 | left PageUp / PageDown | the left window moved 1 → 2 → 1 twice (113-124 per row), rebound cleanly each time | PASS |
| P5 | right PageUp / PageDown | the right window moved 1 → 2 → 3 → 2 → 1 (425-448, 449-472), rebound cleanly each time | PASS |
| P6 | unplug the M-Play with top button 1 held | `mplay: M-Play disconnected; its playback buttons and faders are released`, `mplay/MPA1: mplay lost while held; released executor 101 key`, both windows unbound; on the console StartShow 1 off, no control gestures, nothing unresolved, admission not busy | PASS |
| P7 | replug | `mplay: M-Play (no serial) open`, both windows bound again (60 executors); the operator saw the lights restored as before | PASS |
| P8 | an NX-K unplug in the same run (the wrong cable pulled first) | `nxk: NX-K disconnected … lost with 0 key(s) held`, replug `NX-K open`; the M-Play kept working throughout | PASS (two surfaces independent) |

Not explicitly confirmed by the operator: the exact rest pattern before the first press (top button 1 and the right
section's first two buttons red, both displays `1.1`); it was reported unchanged after the replug.

## Press-to-light timing (12 taps of top button 1 on 101, Flash)

| Transition | n | median | slowest |
| --- | --- | --- | --- |
| down → running | 12 | 152 ms | 170 ms |
| up → stopped | 12 | 133 ms | 246 ms |

Two earlier releases of about 320 ms were outliers. Compared with the M-Touch run (press 218 / 347 ms, release 96 /
150 ms) presses are tighter and releases slightly slower, with 60 executors bound instead of 30.

## Finding

- The timing log reported `not reported stopped within 3 s` for the key held during the unplug: the executor had left
  the binding with its window, so its stop could not be observed. The log now says `no longer bound; not timed` when the
  executor is missing from a fresh context (branch `fix/kb24-mplay-pass`).

Cleanup: `Off Page 1.101`, `Delete Page 1.101 /NoConfirmation`.
