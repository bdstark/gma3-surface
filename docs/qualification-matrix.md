# Qualification matrix: platform, device, firmware (KB-24)

Updated 2026-10-10. One table per axis, each row separating **automated evidence** (stock Lua, Rust and the
real-socket e2e suite, run on any host) from **live qualification** (a recorded run against onPC or with the
device in hand, at a stated revision). Automated evidence never establishes a live row, and a live row holds only
for the console, host and firmware it names. Vocabulary as in [deployments.md](deployments.md): *qualified*,
*harness*, *unqualified*.

Revision of this matrix: service branch `feat/kb24-device-profiles`; vendored modules hardkeys 0.10.0 /
feedback 0.7.0 / control 0.7.0 at `e4741b9` ([VENDOR.md](../tools/ma3/VENDOR.md)); plugin `mtpnxk_surface`
0.11.0 (adds the heartbeat load report; 0.10.0 / protocol 2 is the localhost-only transport without a pairing key; the field is optional).

## Automated suites (what each run establishes)

| Suite | Count | Establishes | Does not establish |
| --- | --- | --- | --- |
| `lua tools/ma3/test/surface_plugin_test.lua` | 243 checks | protocol, pairing, dedup, leases, floods, feedback, context, KB-18 control admission, KB-19 console backend, KB-20 strips, KB-21 binds, KB-23 colours; **KB-24:** the heartbeat's load report (window, budget hit, reset), a key release behind a 31-packet ctl burst applied on the same tick, the per-session rate limit on a ctl burst | console routes, readers or timing |
| `cargo test` in `service/` | 96 tests | framing, link state machine, control transport, strips, banks, lights, device slots, decoders, **KB-24:** key presses/releases and boundaries sent at once while motion saturates the 16-per-tick budget (motion drains in order, nothing aged), the per-kind latency samples, the feedback probe, the load report's parsing, the workload generator | USB, console |
| `sh tools/ma3/test/e2e.sh` | 7 scenes + bench | the real service and the real plugin over a real UDP socket (stock Lua, stubbed console): keys, rotaries, strips, banks, faders, lights, lease expiry, 100-tap bench | console latency, hardware |
| `mtpnxk measure` against the stub ([record](probes/kb-24-measure-macos.md), [report](probes/kb-24-measure-stub-macos.json)) | 15 s mixed workload | the service's own latencies under simultaneous use (key and boundary acks, motion queueing), no loss or reordering at the link, the plugin loop's load under the workload | console feedback or effect latency (the stub's command line and 16 ms frame stand in for the console) |

## Hosts (service host and console)

| Service host | Console | Automated | Live | Evidence |
| --- | --- | --- | --- | --- |
| macOS 26.5.1, Apple silicon (`bdsmbpm401`) | grandMA3 onPC 2.5.1.0 on the same machine, loopback | all suites pass | **qualified** for KB-07 … KB-23 through the `sim` path (keys, rotaries, strips, banks, faders, lights); the physical surfaces through `run` are the KB-24 hardware pass, pending | [deployments.md](deployments.md), `probes/kb-*-surface-macos-2.5.1.md` |
| macOS | onPC on another machine (LAN) | n.a. | unqualified (no latency over a network measured) | KB-08 L7 |
| Windows 11 | onPC 2.5.1 | Rust and Lua suites not run on Windows | unqualified (WinUSB binding for the NX-K not exercised) | — |
| Linux | onPC on macOS/Windows over LAN | not run | unqualified (udev rule needed; no onPC for Linux) | — |
| any | onPC other than 2.5.1.0, physical grandMA3 consoles | n.a. | unqualified / not planned | — |

## Devices and firmware

The devices report no firmware version in their USB descriptors that the service reads (`bcdDevice` is not
recorded); the "firmware" column is the device's product string and what the protocol captures were made against.
A row's live status is the KB-16 protocol-level qualification unless a later record is named.

| Device | VID:PID | Firmware / protocol source | Automated | Live (protocol) | Live (roles through `run`) |
| --- | --- | --- | --- | --- | --- |
| NX-K | `11be:e102` | product "NX-K"; decoder from captures (KB-07) | decoder, LED map, device-slot tests | **qualified** macOS (KB-07: keys and LEDs confirmed by the operator) | keys **qualified** (KB-07/08); rotaries as encoders **sim only** (KB-19): the physical rotaries are the KB-24 hardware pass |
| M-Touch | `11be:f808` | product "M-Touch", no serial; MTouchPlay `e48eb2c` `docs/protocol.md` | 17 decoder/encoder tests, strips 8, banks 18, lights 7 | **qualified** macOS 2026-10-10 ([record](probes/kb-16-hardware-mtouch-macos.md)): every control, report type, LED/bar/display write, unplug and replug | parameter strips, playback keys/faders and lights **sim only** (KB-20 … KB-23 records): physical pass pending |
| M-Play | `11be:f80c` | product "M-Play"; MTouchPlay `e48eb2c` | decoder/encoder tests, banks | **qualified** macOS 2026-10-10 ([record](probes/kb-16-hardware-mplay-macos.md)) | playback buttons/faders and lights **sim only** (KB-21 … KB-23): physical pass pending |
| two surfaces at once (NX-K + M-Touch, + M-Play) | — | — | `usb_surfaces.rs` 3 tests, `main.rs` loss tests | unqualified | unqualified (KB-24 hardware pass) |
| USB removal and return while paired | — | — | device-slot tests (keypad lost: holds released; M-Touch lost: touches ended) | unplug/replug detection **qualified** per device (KB-16) | the released holds at the console: unqualified live |

## Scenarios of the KB-24 end-to-end pass

| Scenario | Automated | Live | Where it will be recorded |
| --- | --- | --- | --- |
| RGB and extended-colour pages on the strips and rotaries | KB-23 Lua/Rust tests (colour sources, mixed values) | **sim** 2026-10-10 (KB-23 surface record: R/G/B, UV, mixed) | hardware pass: `probes/kb-24-hardware-*.md` |
| Pan/Tilt | KB-19 physical-readout tests | **sim** (KB-19 record: Pan at 10 degrees per 10 clicks) | hardware pass |
| mixed fixtures (mixed values, takeover) | strips tests | **sim** (KB-20 record) | hardware pass |
| cue playback, momentary buttons (Temp/Flash), Go+ | banks tests, control module tests upstream | **sim** (KB-22 records: Press/Unpress Page p.e verified) | hardware pass |
| fader pickup and supported fader functions (Master, Temp) | banks pickup tests | **sim** (KB-22) | hardware pass |
| two connected surfaces | device-slot tests | pending | hardware pass |
| USB removal | device-slot tests | detection qualified (KB-16); released holds pending | hardware pass |
| plugin restart | Lua lease/forget tests, e2e lease expiry | **sim** (KB-23: stopping the plugin switched every light to unavailable) | hardware pass (physical lights) |
| console context changes (bank, page, selection) | Lua context tests, Rust stale-generation tests | **sim** (KB-19 … KB-21) | hardware pass |
| stale feedback | lights tests | **sim** (KB-23) | hardware pass |
| latency, missed events, ordering, plugin-loop load under simultaneous use | `measure` against the stub: 15 s, pass except the stub-only artefacts below | **measured** 2026-10-10 (runs A–E against onPC): fails the proposed thresholds, three defects identified | `probes/kb-24-measure-macos.md` |
| continuous-control traffic versus key releases and flood protection | `link.rs` saturation and ceiling tests; Lua burst test; e2e | **regressed live** before the fixes (run D: key acks 236 ms p95, 101 taps superseded, 103 lost); **clean after them** (D2: 67 ms p95, nothing lost, superseded or refused at 100 detents/s + strip + keys) | same |

## Latency and loss thresholds (proposed)

Measured by `mtpnxk measure` ([probes/kb-24-measure-macos.md](probes/kb-24-measure-macos.md) describes the
workload). Thresholds are for a loopback host (service and onPC on one machine). The three latency lines marked
**agreed** were moved on 2026-10-11 from the first proposal to the console's measured cadence plus margin; every
zero-loss line is unchanged. A run on another host publishes its own. A metric without samples is reported `n.a.`
and claims nothing.

| Metric | Definition | Threshold | Why |
| --- | --- | --- | --- |
| key ack p95 / worst | surface key event sent → plugin ack received (service clock) | ≤ 80 ms / ≤ 300 ms | **agreed 2026-10-11**: the plugin tick on 2.5.1 is 13 ms idle and acks sit at 43–47 ms median, 65–73 ms p95 (runs B2–B5); measured plus margin; the retransmit spacing (120 ms) must stay exceptional |
| ctl boundary ack p95 | strip touch / button boundary sent → ack | ≤ 80 ms | as keys (measured 44–50 ms p95) |
| motion queue p95 / worst | a rotary/strip/fader motion queued → sent (never acknowledged, so the service side only) | ≤ 15 ms / ≤ 50 ms | under the 25 ms per-control interval (merging meanwhile) and the 1 ms loop; holds for unsaturated use (C3, E2). Above the per-session ceiling (100 packets/s) deferral is the mechanism and these lines are exceeded by design (D2: 25 / 146 ms, nothing aged); the 250 ms age bound must never be approached |
| feedback p95 | probe key press → first state message in which the probed item changed | ≤ 300 ms | the plugin reads state every frame and sends deltas; a full state at least every second is not the path |
| press-to-effect p95 | plugin `bench`: NUM key event → command-line change (plugin clock) | ≤ 100 ms | KB-08's open item; needs the plugin started with `bench` |
| lost, superseded, plugin-reported ctl loss, aged, stale, overflow, rejected, link downs, deltas ignored | counters over the run | 0 | any loss on loopback is a defect |
| out-of-order refusals | the plugin saw a later per-device sequence first | 0 | ordering must hold on loopback |
| retransmit ratio | retransmitted / events | ≤ 1 % | acks later than 60 ms must be rare |
| feedback probes missed | a probe never saw its change before its Clear 400 ms later | ≤ 10 % of probes | **agreed 2026-10-11**: 3–5 of 39 are missed in every run since round 4 (the console's frame and read timing); zero was not reachable on 2.5.1 |
| plugin tick max | the longest plugin loop iteration in a heartbeat window (plugin 0.11.0) | ≤ 200 ms | **agreed 2026-10-11**: idle spikes of 167 ms recur on 2.5.1 (the console's own work); met in every run but the 100 detents/s saturation (247 ms) |
| plugin ticks at the packet budget | ticks that processed 32 datagrams | 0 | the workload must not approach the per-tick budget |

### Results so far

| Run | Date | Verdict | Notes |
| --- | --- | --- | --- |
| stub (stock Lua, Python 16 ms bridge), 15 s, defaults | 2026-10-10 | every service-side check passes; two **stub artefacts** fail: press-to-effect p95 242 ms (the stub's frame cadence, not a console), plugin-reported ctl loss 372 (every motion is refused `target-unavailable` by a console without an encoder bar, and a refused event is not committed to the control module's per-device sequence, so the next admitted event reports a gap) | [report](probes/kb-24-measure-stub-macos.json) |
| onPC 2.5.1 loopback, plugin 0.11.0 (keyless protocol 2), runs A–E | 2026-10-10 | **FAIL against the proposed thresholds**: admitted motion is clean (run C2: 0 refused/lost/retransmitted, boundaries 23 ms median), but the plugin loop costs 13 ms per tick idle (max 167 ms) and 59 ms with motion and keys, key acks sit at 40–46 ms median, the control queue overflows (`queue-full`) at 40 detents/s plus a strip, and at 100 detents/s the plugin saturates (ticks 180–300 ms) and key taps are lost (superseded 101–234, lost 103–220). The refused-event loss accounting is confirmed live | [record](probes/kb-24-measure-macos.md), `probes/kb-24-measure-onpc-macos-2.5.1-run*.json` |
| onPC 2.5.1 loopback, plugin 0.11.0 + control 0.6.0, after the fixes (runs B2–B5, C3, E2, D2) | 2026-10-11 | **keys and admitted motion pass**: C3 passes every check; E2 and D2 lose nothing (0 lost / superseded / refused / retransmitted at 40 and 100 detents/s with keys; before: 103 lost, 101 superseded, 1957 `queue-full`); key acks 65–69 ms p95 against 80. **Open**: feedback read-back p95 509 ms (11 of 39 probes missed; median 60 ms), plugin tick spikes 266–275 ms, D2 at the datagram budget in 27 ticks and motion deferred by the ceiling (25 / 146 ms) | same record, `…-run{B2,B3,B4,B5,C3,E2,D2}-*.json` |
| onPC 2.5.1 loopback, plugin 0.11.0 + control 0.7.0 + feedback 0.7.0, round 4 (B6, C4, E3, D3) | 2026-10-11 | **C4 and E3 pass**; D3 (100 detents/s + strip + keys) fails only the motion-queue line by design under the ceiling (44 ms worst, was 146; the plugin never at its datagram budget, was 27 ticks); B6: feedback median 55 ms, p95 165 ms (was 60 / 509), 3 probes missed (was 11), press-to-effect p95 122 ms against 100. Scheduling diagnostics: one coroutine, no overlapping ticks, coherent phase sums | same record, `…-run{B6,C4,E3,D3}-*-round4.json` |
| onPC 2.5.1 loopback, round 5 (B7: the state read on the tick after a key event) | 2026-10-11 | press-to-effect p95 **66 ms** (was 122), feedback median 49 / p95 172 ms, key acks 69 ms p95, all zero loss; 5 of 39 probes missed (13 %, line 10 %) | same record, `…-runB7-keys-round5.json` |
| onPC 2.5.1 loopback, after review round 1 of PR #9 / #28 (B8, C6, E5, D5) | 2026-10-11 | **B8, C6, E5 pass every check**; D5 (100 detents/s + strip + keys) fails the motion-queue line by design under the ceiling and the plugin tick max (247 ms, mean 132 ms: the reporting ticks are counted now); no loss, no reordering, no re-pairing in any run | same record, `…-run{B8,C6,E5,D5}-*-review1.json` |

The onPC runs confirmed the second artefact as a defect of the vendored control module's accounting and
identified two more (the control queue overflowing at realistic rates, the plugin loop saturating at 100 detents/s
so that key taps were lost). All three were fixed on 2026-10-11 (control 0.6.0 upstream and re-vendored; the
service's motion interval, ceiling, retransmit spacing and supersede rule; the plugin's work per tick, state cadence
and read budget) and re-measured: see the record's run 3. What stays open is the feedback read-back tail and the
plugin's tick spikes under heavy motion.
