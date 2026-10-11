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
| Plugin | `mtpnxk_surface` 0.10.0 under stock Lua 5.4 behind `tools/ma3/test/udp_pipe_bridge.py` (16 ms frame, `control=fake bench`), stubbed console (no encoder bar, no executors) |
| Service | release build of `feat/kb24-device-profiles` (uncommitted at the time of the run; the committed revision is named in KEYBOARD.md "KB-24") |
| Command | `mtpnxk --plugin 127.0.0.1:9818 --key <hex> measure --seconds 15 --json docs/probes/kb-24-measure-stub-macos.json` |
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

## Run 2: onPC 2.5.1 over loopback (pending)

Procedure, once the plugin 0.10.0 is installed on the console (the heartbeat load report needs it; an older
plugin gives `plugin loop: no load report`):

1. Start the plugin with `bench` for the press-to-effect figures:
   `Plugin "mtpnxk_surface" "key=<hex> input=keyboard control=console force bench"` (the `force` is needed
   while the MCP bridge has input enabled; the bridge must hold no keys).
2. Select fixtures and show an encoder bank (so slots 1–4 admit motion), put executors on 101, 201 and 301 of
   the current page if the playback part of the workload is wanted (or pass `--no-playback`).
3. Run, from the service directory:

   ```bash
   ./target/release/mtpnxk --key-file <path> measure --seconds 30 --json ../docs/probes/kb-24-measure-onpc-macos-2.5.1.json
   ```

4. Record the report here (host, onPC version, show, revision, every metric, the verdicts), the state of the
   plugin-reported-loss finding, and clear the programmer afterwards (`ClearAll`: the rotary motion moved the
   selection's attributes).

Run 2 was not executed in the session that wrote this record: the pairing key of the running plugin is not in
the repository or a known file, and reading it out of the console's Lua state was declined.
