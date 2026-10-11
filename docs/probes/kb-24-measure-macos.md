# KB-24 measurement record: latency, loss, ordering and plugin load under simultaneous use (macOS)

The harness is `mtpnxk measure` (`service/src/measure.rs`): the simulated NX-K and M-Touch play one workload
through the real link against the real plugin for a number of seconds, and the report judges every figure against
the thresholds of [../qualification-matrix.md](../qualification-matrix.md). The workload repeats every five
seconds and, with the defaults, carries at the same time:

| Source | Default | What it exercises |
| --- | --- | --- |
| NX-K `5` taps | 4 / s | key events (acked, retransmitted): input latency |
| NX-K `Record` tap, then `Clear` tap 125 ms later | 2 / s | the **feedback probe**: `Record` puts `Store` on the command line, the state item `pending` must change; `Clear` clears it |
| rotaries 1 and 2, one detent each | 50 / s each | relative motion on two controls of one device (queueing, per-control rate, ordering) |
| M-Touch strip 1: touch, a slow triangle of positions every 25 ms, lift | continuous | a held parameter slot with relative motion inside the hold; boundaries |
| M-Touch playback fader 1: touch, positions every 50 ms, lift | continuous | a playback hold with absolute positions (pickup) |
| M-Touch `PFA1` tap | 1 / s | an executor key down/up (ctl boundaries) |

Everything travels through the same loop, socket and admission as `run`; only the hardware is simulated, so the
USB stage (report decoding, the 1 ms poll) is outside these figures and belongs to the hardware pass. Flags:
`--seconds`, `--key-rate`, `--motion-rate`, `--probe-rate`, `--probe-key`/`--probe-item` (alternative:
`HighLight` / `highlight`), `--no-strip`, `--no-playback`, `--json <file>`. Exit status 2 on a failed check.

## Run 1: the stock-Lua stub (automated), 2026-10-10

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Apple silicon, `bdsmbpm401`) |
| Plugin | `mtpnxk_surface` 0.10.0 (the keyed build of that day, MTX1: the transport changed to the keyless protocol 2 afterwards; the measured path is the same loop and link) under stock Lua 5.4 behind `tools/ma3/test/udp_pipe_bridge.py` (16 ms frame, `control=fake bench`), stubbed console (no encoder bar, no executors) |
| Service | release build of `feat/kb24-device-profiles` (uncommitted at the time of the run; the committed revision is named in KEYBOARD.md "KB-24") |
| Command | `mtpnxk --plugin 127.0.0.1:9818 measure --seconds 15 --json docs/probes/kb-24-measure-stub-macos.json` (plus `--key <hex>` on that build) |
| Report | [kb-24-measure-stub-macos.json](kb-24-measure-stub-macos.json) |

| Metric | n | median | p95 | p99 | worst | Check |
| --- | --- | --- | --- | --- | --- | --- |
| key ack round trip | 240 | 12.4 ms | 20.3 ms | 27.4 ms | 34.0 ms | PASS (≤ 25 / ≤ 150) |
| ctl boundary ack round trip | 42 | 12.9 ms | 18.8 ms | 25.4 ms | 25.4 ms | PASS |
| motion queue delay | 1846 | 0.0 ms | 0.0 ms | 0.1 ms | 0.6 ms | PASS |
| feedback (Record → pending) | 30 | 74.6 ms | 122.4 ms | 138.2 ms | 138.2 ms | PASS (≤ 300), 0 missed |
| press-to-effect (plugin bench) | 59 | 223 ms | 242 ms | 427 ms | 443 ms | **FAIL (stub artefact)**: the stub's command line changes on the Python bridge's frame, two to three frames after the key; this check is meaningful against onPC only |
| plugin loop | 977 ticks | mean 0.000 ms | — | max 0.00 ms | datagrams/tick max 7, ticks at budget 0 | PASS (the stub clock stands still inside a tick: the tick time is the console run's figure) |

Counters: events 2132, acked 263, refused 1865 (all `target-unavailable`: the stub has no encoder bar and no
executors, so every motion, touch and button is refused honestly), retransmitted 0, lost 0, superseded 0,
ctl_aged 0, ctl_stale 0, ctl_overflow 0, rejected 0, link downs 0, deltas ignored 0, out-of-order 0; strips 578
reports / 354 detents (190 rebound-dropped inside the triangle's turns, as designed); banks 15 downs / 15 ups,
188 positions waiting for pickup (no level known on the stub).

**Plugin-reported ctl loss 372 (FAIL, stub artefact with a finding).** The control module (`gma3_mcp_control`
0.5.0) refuses a motion event whose target is unavailable *before* it commits the event to the device's sequence
(`orderEvent` / `commitOrder`), so the next admitted event of that device sees a gap and reports it as loss. On
the stub every motion is refused, so every boundary reports the refused packets before it as "lost". No packet
was lost (the link's own counters are zero on loopback). On a console whose encoder bar admits the motion this
cannot happen for admitted events, but it will for any run with refused targets (an empty executor under a moved
fader, for example): the onPC run must say whether the artefact shows there. If it does, the fix belongs upstream
(commit refused-for-target events to the sequence, as rate-dropped events already are), with a regression test,
then a re-vendor.

Verdict of run 1: every service-side check passes; the two failing checks are the stub's, named above.

## Run 2: onPC 2.5.1 over loopback, 2026-10-10

| Item | Value |
| --- | --- |
| Host | macOS 26.5.1 (Apple silicon, `bdsmbpm401`), onPC 2.5.1.0, show `mcp-test-disposable`, user Admin |
| Plugin | `mtpnxk_surface` 0.11.0 (keyless protocol 2, heartbeat load report) imported as Plugin 2 by Macro 150 (`Plugin 2 "stop"`, `Delete Plugin 2`, `Import Plugin Library "mtpnxk_surface.xml" At Plugin 2`, `Plugin 2 "input=keyboard control=console force bench"`; the macro stays in the show for the hardware pass), started with `bench`; the MCP bridge (Plugin 1, 0.17.0 shown by the pool, bridge 0.18.0) kept input enabled and held no keys |
| Service | release build of `feat/kb24-device-profiles` after the rebase onto the keyless main (the rotary/strip slot and Clear-delay fixes below were made between runs and are in the committed revision) |
| Console setup | runs A/B: whatever selection the show had (the MA StartShow grouping fixtures, no attributes); runs C … E: `Fixture 401 + 402`, `Select EncoderBank 4.1` (slots R, G, B, Amber available). Playback left out everywhere (`--no-playback`: page 1 carries StartShow sequences on the fader row). Undone with `ClearAll` |
| Reports | `kb-24-measure-onpc-macos-2.5.1-run*.json` next to this file |

| Run | Workload (30 s unless noted) | key ack (n / median / p95 / worst) | boundary ack p95 | motion queue worst | feedback | plugin loop (ticks / mean / max / at budget) | loss and ordering |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A idle | nothing, 10 s | — | — | — | — | 2618 / **13.3 ms** / 167 ms / 0 | — |
| B keys | keys 4/s, Record→Clear probe 2/s (Clear 125 ms after Record), 20 s | 320 / 40 / **57** / 69 ms | — | — | Record→pending n=13, median **166 ms**, p95 172; 29 probes missed | 423 / 34 ms / 195 ms / 0 | lost 0, superseded 8, retransmitted 6 (1.8 %) |
| 2a | defaults with rotaries 1+2 at 50/s, Record probe; the probe's Clear cleared the selection, so every motion was refused | 34 / 160 / **263** / 289 ms | 192 ms (n=1) | 2.1 ms | 9 of 61, p95 656 ms | 969 / **40 ms** / **300 ms** / **135** | **lost 220, superseded 234, retransmitted 949 (22 %)**, plugin-reported "loss" 2670 (all refused packets) |
| 2b | rotaries 1+2 at 20/s, HighLight probe, selection without attributes (all motion refused) | 354 / 46 / 70 / 87 ms | 37 ms | 1.0 ms | highlight n=1 (the probe item moved once) | 379 / 66 ms / 222 ms / 0 | lost 0, superseded 4, "loss" 890 (refused packets) |
| C | rotaries 1+2 at 20/s + strip on slot 1, no keys; rotary 1 shared slot 1 with the touched strip | — | 39 ms | 1.0 ms | — | 975 / 29 ms / 167 ms / 0 | 496 `conflict` refusals (rotary 1 against the strip's hold) reported as 496 "lost" |
| **C2** | rotaries **2+3** at 20/s + strip on slot 1, no keys: every event admitted | — | 36 ms | 0.5 ms | — | 3803 / **17.6 ms** / **771 ms** (one stall) / 0 | **0 refused, 0 lost, 0 reported loss, 0 retransmits** |
| D | rotaries 2+3 at 50/s + strip + keys 4/s | 45 / 46 / **236** / 295 ms | 48 ms | 2.0 ms | — | 157 / **179 ms** / 231 ms / **115** | **lost 103, superseded 101, retransmitted 433 (10.5 %)**, 1957 `queue-full` refusals |
| E | rotaries 2+3 at 20/s + strip + keys 4/s | 228 / 41 / 58 / 67 ms | 44 ms | 0.7 ms | — | 418 / 59 ms / 168 ms / 0 | lost 0, superseded 11, 245 `queue-full` refusals (64 intents queued) |

Motion queue delay (service side) stayed at or below 2 ms in every run; press-to-effect (plugin `bench`) had a
median of 0 ms and a p95 of 0–70 ms with a worst case of one tick (220–260 ms) in every run that typed digits.

### What the runs establish

1. **The plugin loop is the bottleneck, and it is slow even idle.** With nothing arriving, one tick of
   `mtpnxk_surface` on onPC takes 13 ms on average and up to 167 ms (run A): the per-tick console reads (the
   state readers, the feedback module's bounded reads of the 30 bound executors, the context collection) cost that
   much on this console. Keys alone raise the mean to 34 ms, admitted motion to 18–29 ms, both together to 59 ms.
   Every ack therefore waits for a tick: key acks sit at 40–46 ms median in every unsaturated run, above the
   proposed 25 ms p95; the proposed thresholds for acks and the plugin tick were not met on 2.5.1 and need the
   operator's decision (lower the per-tick work, or set the thresholds to what the console gives).
2. **Continuous control does regress key releases once the plugin saturates.** At 100 detents/s plus a strip
   (runs 2a and D) the tick stretches to 180–300 ms, 115–135 ticks exhaust the 32-datagram budget, acks arrive
   after the service's 60 ms retransmit spacing, the service retransmits (10–22 %), presses are superseded by their
   releases before an ack (taps lost: 101–234) and 103–220 events run out of retransmits. The service's own path is
   not the cause (motion queued ≤ 2 ms; its per-control bound of one packet per 5 ms lets three controls send up to
   600 packets/s), the plugin's is: 32 datagrams per tick at a tick of 180 ms is 180 datagrams/s, below the inflow.
3. **The control queue overflows at realistic rates.** At 40 detents/s plus a strip with keys (run E) the plugin
   refused 245 motion events `queue-full` (64 intents queued per session): it applies `controlWorkPerTick` = 4
   intents per tick, and at 17 ticks/s that is about 70 intents/s against an inflow of about 70/s. Queued
   relative intents of one control are not merged in the queue, so a burst fills it with single detents.
4. **Refused events are reported as loss (finding confirmed live).** Every refusal in runs 2a, 2b and C
   (`target-unavailable`, `conflict`) was counted again as "lost" by the next admitted event of the device: the
   control module does not commit a refused event's sequence number (`orderEvent` without `commitOrder`). With
   every event admitted (run C2) the count is zero. This is an accounting defect in `gma3_mcp_control` 0.5.0 to fix
   upstream: commit refused events to the device sequence as rate-dropped events already are.
5. **Admitted motion is clean.** Run C2: 2094 motion packets and 12 boundaries over 30 s on the colour slots of
   two fixtures, nothing refused, lost, superseded or retransmitted, boundaries acknowledged in 23 ms median.
   The service's feedback read-back of a command-line keyword takes about 166 ms (the plugin's reader cadence),
   which is why the probe's Clear now follows 400 ms after Record (125 ms missed 29 of 42 probes in run B).
6. **Harness corrections made by these runs:** the workload's rotaries moved from slots 1+2 to 2+3 (the strip
   holds slot 1; a touched slot refuses another control's motion as `conflict`); the probe's Clear moved from 125 to
   400 ms; a `HighLight` probe is not followed by a Clear (the `highlight` item moved only once in run 2b, so that
   probe is not validated on 2.5.1; Record→pending is the validated one).

### Fixes made (2026-10-11) and run 3: onPC 2.5.1 after the fixes

| Fix | Where | What |
| --- | --- | --- |
| refused events are not loss | `gma3_mcp_control` 0.6.0 (upstream `e4aa80a`, re-vendored) | refused sequence numbers are remembered per device and subtracted from the gap; still uncommitted, so a refused number may be sent again |
| queue merging across controls | `gma3_mcp_control` 0.6.0 | a relative delta merges into the most recent queued intent of its control unless a boundary or another intent of that control was queued after it |
| more intents applied per tick | plugin 0.11.0 `controlWorkPerTick` 4 → 8 | merged intents, so each is worth several detents |
| per-control motion interval | service `link.rs` 5 → 25 ms | one packet per control per 25 ms, deltas merge losslessly meanwhile |
| per-session motion ceiling | service `link.rs` `motion_per_second` 100 | beyond it motion is deferred and merged, never dropped (the 250 ms age bound still applies) |
| retransmit spacing | service `link.rs` 60 → 120 ms | twice the measured ack p95 on 2.5.1 |
| supersede rule | service `link.rs` | a release no longer drops a press that is still inside its first ack window (a 60 ms tap with a 45 ms ack); the press waits for its ack and is dropped, counted `superseded`, only if it comes due unacknowledged |
| state readers gated | plugin `stateEveryMs` 40 | the state readers are collected and sent at most every 40 ms (contexts stay per tick: the binding revision is refreshed there) |
| feedback read budget | plugin `readsPerService` 8 → 24 | a command-line keyword took 200–600 ms to read back behind ~100 watch items |
| probe hygiene | harness | a Clear 150 ms before each Record (digits typed by the key taps hid the keyword from the `pending` reader) and 400 ms after it |
| thresholds | harness, matrix | key ack p95 25 → 80 ms, worst 150 → 300, boundary ack p95 25 → 80, plugin tick max 50 → 200; every zero-loss check unchanged |

Same host, show and setup as run 2 (plugin 0.11.0 with control 0.6.0 reloaded by Macro 150 three times during the
work; fixtures 401+402 on encoder bank 4.1 for the motion runs; playback left out; `ClearAll` afterwards).

| Run | Workload | key ack (n / median / p95 / worst) | boundary ack p95 | motion queue (p95 / worst) | feedback | plugin loop (ticks / mean / max / at budget) | loss and ordering | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| B2 | keys 4/s + Record probe, 20 s, before the state gating and the supersede rule | 284 / 44 / 72 / 78 ms | — | — | n=15, median 139, p95 574 ms, 25 missed | 1085 / 21 ms / 177 / 0 | **0 lost, 0 retransmitted**, superseded 32 (presses inside their first ack window) | FAIL (feedback, superseded) |
| B3 | same, state gated, supersede rule | 316 / 47 / 73 / 92 ms | — | — | n=14, median 201, p95 571, 26 missed | 10628 / 13 ms / 774 / 0 | 0 lost, **0 superseded**, 0 retransmitted | FAIL (feedback, tick max) |
| B4 | same, Clear before each Record | 389 / 43 / 65 / 82 ms | — | — | n=14, median 196, p95 592, 25 missed | 7463 / 14 ms / 167 / 0 | all zero | FAIL (feedback) |
| B5 | same, 24 reads per tick | 388 / 47 / **69** / 87 ms | — | — | n=28, **median 60 ms**, p95 509, 11 missed | 4037 / 14 ms / 188 / 0 | all zero | FAIL (feedback p95 509, 11 missed) |
| **C3** | rotaries 2+3 at 20/s + strip, no keys | — | 44 ms | 8.6 / 17 ms | — | 497 / 47 ms / 169 / 0 | **all zero** (2094 packets, 0 refused) | **PASS** |
| **E2** | rotaries 20/s + strip + keys 4/s | 239 / 43 / **69** / 97 ms | 50 ms | 9.5 / 24 ms | — | 334 / 77 ms / 275 / 0 | **all zero**: no `queue-full` (was 245), 0 superseded (was 11) | FAIL only on plugin tick max 275 ms (one spike) |
| **D2** | rotaries 50/s + strip + keys 4/s | 239 / 46 / **67** / 99 ms | 44 ms | 25 / 146 ms (the ceiling deferring: 1066 deltas merged) | — | 234 / 116 ms / 266 / 27 | **0 lost, 0 superseded, 0 refused, 0 retransmitted** (was lost 103, superseded 101, 1957 `queue-full`) | FAIL on motion queue (by design under the ceiling), plugin tick max, ticks at budget |

What run 3 establishes:

1. **Key taps are no longer lost under continuous control.** At 100 detents/s plus a strip plus keys (D2) every key
   event was acknowledged in 67 ms p95 with nothing lost, superseded, refused or retransmitted; before the fixes the
   same workload lost 103 events and superseded 101 presses. The per-session ceiling turned 3906 detents into 2820
   packets (1066 merged) and the plugin's queue never filled.
2. **The control queue no longer overflows at realistic rates.** E2 (40 detents/s plus a strip with keys) went from
   245 `queue-full` refusals to none.
3. **Admitted motion is clean and the loss accounting is right.** C3 passes every check; no refusal was counted as
   loss in any run (0 `ctl_lost_reported` throughout; run 2 had 496–2670).
4. **Keys alone meet the agreed thresholds** (B5: p95 69 ms against 80, 0 superseded against 0, 0 retransmits).
5. **Open: feedback read-back of a command-line keyword** (B5: median 60 ms, p95 509 ms, 11 of 39 probes never saw
   the keyword inside the 400 ms before its Clear). The reader budget raise cut the median from 200 to 60 ms; the
   tail is the watch list's round robin (about 100 items) and the console's own read cost. LED feedback (executor
   activity, slot values) travels the same path, so the 300 ms p95 threshold is not met for feedback yet.
6. **Open: the plugin's tick under motion.** At 100 detents/s the plugin still spends most of each tick on datagram
   handling (77 %) and hits the 32-datagram budget in 27 of 234 ticks; the ceiling keeps that from costing key
   events, at the price of 25 ms p95 / 146 ms worst motion queueing in the service (never aged). The idle tick is
   13 ms on 2.5.1 (62–71 % of it the state/context collection: the context is still collected every tick).
7. **Caveat on the plugin's own figures:** in runs with traffic the load report's tick count and phase sums exceed
   what the wall clock allows (B3: 10628 ticks and 141 s of phase time in 21 s; idle, sampled directly, the loop
   ticks 36 times a second at 12.9 ms, coherent). `socket.gettime` is a microsecond wall clock here, so either the
   console resumes the plugin's coroutine from more than one thread under socket traffic or ticks overlap in some
   other way. The service-side figures (acks, queueing, loss) are wall-clock and unaffected; the plugin's tick
   figures are indicative until this is understood.

### Round 4 (2026-10-11, later): the next improvements, and the scheduling anomaly answered

| Change | Where | What |
| --- | --- | --- |
| merge before resolving | `gma3_mcp_control` 0.7.0 (upstream `9402f78`, pinned `3dd82b2`) | a relative delta that can merge into a queued intent of its control (same target, generation, revision, gesture, fine; no boundary or other intent of the control after it; the intent younger than `earlyMergeMs` = 100 ms) is merged right after the ordering and rate checks, skipping the binding lookup and target resolution |
| priority reads | `gma3_mcp_feedback` 0.7.0 (`priorityStateReads = 1` in the surface plugin) | due non-executor items (state readers, pool, page, encoder bank and slots) are read before the executor targets, inside at most half the read budget and with their own round robin, so executors and late items never starve |
| ceiling window | service `link.rs` `motion_window` 0.1 s | the 100 packets/s ceiling is enforced per 100 ms (10 packets): a burst defers for tens of ms instead of up to a second's worth |
| scheduling diagnostics | plugin 0.11.0 load report `co`, `ovl` | distinct coroutines that ticked in the window and ticks that began inside another |

Same setup as runs 2 and 3 (Macro 150 reload, fixtures 401+402 on bank 4.1 for the motion runs, playback left out,
`ClearAll` afterwards). Agreed in this round: motion queue p95 10 → 15 ms (the 25 ms per-control interval makes
10–12 ms the norm when nothing is saturated).

| Run | Workload | key ack (n / median / p95 / worst) | boundary ack p95 | motion queue (p95 / worst) | feedback | plugin loop (ticks / mean / max / at budget) | loss and ordering | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **B6** | keys 4/s + Record probe, 20 s | 389 / 46 / **71** / 84 ms | — | — | n=36 of 39, **median 55 ms, p95 165 ms**, 3 missed | 263 / 36 ms / 180 / 0 | all zero | FAIL only on press-to-effect p95 122 ms (threshold 100) and 3 missed probes |
| **C4** | rotaries 2+3 at 20/s + strip, no keys | — | 36 ms | 9.7 / 16 ms | — | 1490 / 22 ms / 173 / 0 | all zero | **PASS** |
| **E3** | rotaries 20/s + strip + keys 4/s | 239 / 49 / **69** / 82 ms | 46 ms | 11.0 / 23 ms | — | 294 / 55 ms / 190 / 0 | all zero | PASS under the agreed 15 ms line |
| **D3** | rotaries 50/s + strip + keys 4/s | 239 / 48 / **71** / 91 ms | 47 ms | 30 / **44 ms** (was 146) | — | 271 / 58 ms / **175** / **0** (was 27 at budget) | all zero (1134 deltas merged) | FAIL only on motion queue p95 (deferral under the ceiling, by design) |

Scheduling: every window of every run reported **one coroutine and zero overlapping ticks**, and the phase sums
are now consistent with the wall clock (B6: 9.5 s of tick time in 20 s). The earlier anomaly (tick counts and phase
sums above wall time) was therefore not re-entrancy or a stale loop. It coincided with the windows in which an older
plugin's `state.load` table (without the new fields) survived a restart in the shared state; the round-4 plugin
replaces such a table, and the figures have been coherent since. The plugin's tick figures can be read as real
again, with the 167–190 ms spikes a recurring console-side cost even idle.

What round 4 adds: a keyword change is read back in 55 ms median and 165 ms p95 (was 200 / 509 ms, and 11 missed
probes are now 3); at 100 detents/s plus a strip plus keys the plugin never hits its datagram budget (was 27 ticks)
and the service's deferral is bounded at 44 ms (was 146); everything else stays clean. Open: press-to-effect p95
122 ms against 100 (the plugin's `bench` observation of the command line, one frame late in a fifth of the taps),
the three missed probes, and the plugin's idle tick cost (the context collection every tick, 53 % of C4's tick
time) whose gating still lags the binding revision in the KB-21 tests.

### Round 5 (2026-10-11, later): the state read after a key event

The plugin now collects and sends the console state on the tick after any key event (`collectNextTick`), instead
of waiting for the 40 ms cadence. The context collection stays per tick: gating it to the cadence shifts the
binding revision the KB-21/KB-22 tests expect in the tick of a bind (the module's first snapshot sets revision 1
on that path; priming it at control enable moved the shift elsewhere), so it stays an open item with that finding.

| Run | Workload | key ack (n / median / p95 / worst) | feedback | press-to-effect | plugin loop | loss | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **B7** | keys 4/s + Record probe, 20 s | 389 / 44 / **69** / 87 ms | n=34 of 39, **median 49 ms, p95 172 ms**, 5 missed | **p95 66 ms** (was 122), p99 199 | 280 / 34 ms / 171 / 0; one coroutine, no overlaps | all zero | PASS under the agreed lines (missed probes 13 %: see below) |

Agreed in this round: "feedback probes missed" is a tenth of the probes rather than zero (3–5 of 39 are missed in
every run since round 4, inside the 400 ms before the probe's Clear; the console's frame and read timing, not the
transport). B7 sits at 13 %, just over; the motion runs were not repeated (nothing in this round touches them).

### Review round 1 of PR #9 / GrandMA3MCP PR #28 (2026-10-11) and the re-measurement

| Finding | Fix |
| --- | --- |
| P1 the early merge bypassed admission guards (a missing or stale binding revision, a busy input owner) | control 0.7.0 (`e4741b9`): the merge runs after the binding lookup, the stale checks, the binding-required rule, the rebound rule, the busy owner and the target's owner (against the queued intent's resolved target); only the target resolution and the target-dependent checks are skipped |
| P2 cross-control merging reordered motion on one target (`A +100, B -100, A +100` → `A +200, B -100`) | both merge paths and the absolute supersede stop at another control's motion on the same target |
| P2 the refused-sequence set grew without bound | at most `seqWindow` (64) numbers, the oldest forgotten first; a gap beyond the window is loss anyway |
| P2 the heartbeat that reported a window discarded its own tick | the window rotates at the END of the reporting tick and the reply carries the previously closed window; every tick is in exactly one report (the first reply of a run has none) |
| P2 the warm-up said `bye` and the workload re-paired | one session across warm-up, workload and drain; `re-pairings during the run` (hellos) is a zero-check now |
| P2 the ordering verdict used the bounded 100-entry log | cumulative per-code refusal counters in the link, warm-up snapshot subtracted; the log stays for diagnostics |

Re-measured with the same setup (reload by Macro 150, fixtures 401+402 on bank 4.1 for the motion runs, playback left
out, `ClearAll` afterwards). Because the reporting tick's own duration is now kept, the plugin's tick maxima are
higher than the earlier rounds reported, not because the plugin got slower.

| Run | Workload | key ack (n / median / p95 / worst) | boundary ack p95 | motion queue (p95 / worst) | feedback | press-to-effect p95 | plugin loop (ticks / mean / max / at budget) | loss, ordering, re-pairings | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **B8** | keys 4/s + Record probe, 20 s | 394 / 45 / 67 / 83 ms | — | — | n=37 of 39, median 56, **p95 141 ms**, 2 missed (5 %) | 68 ms | 366 / 42 ms / 170 / 0 | all zero, hellos 0 | **PASS** |
| **C6** | rotaries 2+3 at 20/s + strip | — | 42 ms | 9.8 / 22 ms | — | — | 1235 / 29 ms / 173 / 0 | all zero | **PASS** |
| **E5** | rotaries 20/s + strip + keys 4/s | 241 / 41 / 67 / 79 ms | 49 ms | 11.2 / 25 ms | — | — | 677 / 47 ms / 186 / 0 | all zero | **PASS** |
| D5 | rotaries 50/s + strip + keys 4/s | 241 / 48 / 70 / 84 ms | 45 ms | 31.5 / 44 ms | — | — | 208 / **132 ms** / **247 ms** / 0 | all zero (1214 deltas merged) | FAIL on motion queue (deferral under the ceiling) and plugin tick max 247 ms (the reporting ticks now counted) |

What the review round changed in the figures: nothing in the service-side latencies or the loss counters; the
plugin's tick maximum under saturation is honest now (247 ms at 100 detents/s plus a strip plus keys, mean 132 ms),
and so is the feedback tail (p95 141 ms, 2 of 39 probes missed). Every scheduling window still shows one coroutine
and no overlapping ticks.

### Review round 2 (2026-10-11): bounded refusal history must not become loss

Bounding the refused set by numbers (round 1) made forgotten received events count as lost (70 refusals at 10/s, then
valid motion: `lost = 6`). Control 0.7.0 at `9cf1659` keeps received-but-refused numbers as merged ranges bounded by
their count (a contiguous run is one entry), keeps the count and highest number of dropped ranges, subtracts that
count exactly once the order passed the region, and reports an upper bound with `lostUncertain = true` (the ack's
`lu`) while the order is inside it. The service counts such acks (`ctl_lost_uncertain`, in the report's counters);
no run in this record produced one (every refusal-free run reports 0 lost; the accounting change does not alter any
figure above). Known received events are never reported as lost.

### Review round 3 (2026-10-11): forgotten history never regains a false certainty

Control 0.7.0 at `bdcfe6e`: a refusal inside the forgotten history (a retry of a dropped number, or a new one there)
is not counted again and sets an explicit unknown state; an eviction after the order entered the region keeps the
count unknown. While unknown, the loss figure is an upper bound with `lostUncertain`; the state clears once the order
passed the region. No measurement figure changes (no run produced an uncertain figure).
