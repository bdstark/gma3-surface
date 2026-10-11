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

### Recommended fixes (not made here; they are shared-module and design decisions)

- Service (`link.rs`): widen the per-control motion interval (5 ms → about 25 ms, merging deltas losslessly) and
  add a per-session ceiling well under the plugin's measured capacity, so that four controls at full speed cannot
  exceed about 100 packets/s.
- Control module (`gma3_mcp_control`): merge queued relative intents of one control instead of queueing single
  detents; commit refused events to the device sequence (finding 4); raise `controlWorkPerTick` once merged.
- Plugin loop: measure the per-tick cost of the feedback reads on 2.5.1 (idle 13 ms mean, spikes to 167–771 ms)
  before any threshold on acks is agreed; the spikes also delay key acks past the 60 ms retransmit spacing.
