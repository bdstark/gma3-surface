# KB-07, KB-08 and KB-15 to KB-23 in mtpnxk: the surface consumer and its qualification


> Transport update: plugin 0.10.0 / protocol 2 is localhost-only and has no pairing key. Older key, MAC and LAN references below describe historical work, not current setup. See [operator guide](docs/operator-guide.md).

Updated: 2026-10-10 (KB-23 surface half added, see the last section). The feature series KB-01 to KB-09 is defined and tracked in
[bdstark/gma3-mcp `KEYBOARD.md`](https://github.com/bdstark/gma3-mcp/blob/main/KEYBOARD.md).
KB-01 to KB-06 are complete there (macOS, onPC 2.5.1.0) and provide the reusable
console modules this repository vendors. KB-07 (the surface consumer) and KB-08
(documentation and qualification of the surface) live here; KB-09 (shortcut-table
cache, operator-managed profile shortcuts) is module work in the MCP repository.

Direction change (2026-10-09): the consumer is a **cross-platform Rust service**
on the PC, not Pico firmware. The Pico tree is archived under
[legacy/pico](legacy/pico/ARCHIVED.md) and tagged `pico-firmware-final`.

Operator documents produced by KB-08: [docs/operator-guide.md](docs/operator-guide.md),
[docs/deployments.md](docs/deployments.md), [docs/kb-08-acceptance.md](docs/kb-08-acceptance.md).

## KB-07 — Integrate the independent mtpnxk surface consumer

**Request:** As a surface user, I want responsive keypad input and trustworthy
LED state across network interruptions.

**Depends on:** KB-03, KB-04, KB-06 (vendored). No MCP bridge change was
needed. One reusable-module change was made in the MCP repository and
re-vendored rather than forked: `gma3_mcp_hardkeys` 0.5.0 resolves any
`Enums.VirtualKeyCode` name through the shortcut table (the NX-K's Edit, Copy,
HighLight, … keys) and lets the consumer prefer the row of a same-target tie
(`+ - . /` on the keypad row), with regressions in that repository's harness
(PR #12, merged as `6e0d9c1`).

**Design:** [docs/surface-protocol.md](docs/surface-protocol.md). The six
decisions it settles, in the order they were asked for:

1. Feedback schema reconciled against the KB-06 readers: tri-state items
   (`0`, `1`, `"?"`), derived `pending` and `preview`, units, unknown/stale
   rules, protocol and module versions (section 1).
2. Pairing and packet semantics: a SipHash-2-4 MAC over every datagram under
   the first 16 bytes of a 32-byte pairing key (HMAC-SHA256 was the first
   choice and cost 20 ms per packet in onPC's Lua), fresh random session ids,
   strictly increasing sequence numbers, event ids with retransmission
   (4 × 60 ms), acks, lease and forget times, validation before any module
   call (section 2).
3. Event delivery separated from held-key reconciliation: duplicates
   deduplicated, superseded events refused, releases owner-scoped, heartbeats
   release lost releases and never press, nothing replayed after either side
   restarts, unresolved releases kept as records across restarts (section 3).
4. Loop budget: deadlines first, feedback reads spread over frames, bounded
   packets per frame, per-session rate limit, auth-failure throttle (section 4).
5. Freshness: plugin generation + feedback epoch, stale and identity-uncertain
   values rendered unknown, console state kept apart from the surface's own
   holds (section 5).
6. Exclusive use as an operating rule, with a courtesy check against the MCP
   bridge that is explicitly not arbitration (section 6).

Measurable limits (press latency, ack round trip, cleanup, LED freshness,
stale indication, flood resilience) are chosen in section 8, which now also
carries each limit's measured status.

### Implementation

| Part | Where | Verified by |
| --- | --- | --- |
| Surface plugin (Lua, runs in onPC) | [tools/ma3/mtpnxk_surface.lua](tools/ma3/mtpnxk_surface.lua) 0.1.0, [mtpnxk_surface.xml](tools/ma3/mtpnxk_surface.xml) | `lua tools/ma3/test/surface_plugin_test.lua` — 107 checks under stock Lua with stubbed console and socket: SipHash vectors, pairing/replay/MAC rejection, press/release/dedup/old-seq/reordering/superseded, duplicate acks replaying the original outcome, unsupported keys, heartbeat reconciliation (lost release released, lost press reported not pressed), lease expiry/revival/forgetting, restart via new hello, capacity, per-session rate limit, a 10 000-datagram flood not starving a lapsed lease, feedback deltas/full/unknown/stalled reader/epoch bump/identity uncertainty, keyboard backend calls, bench mode, the bridge courtesy check, two independent instances, allow list, kept records adopted across a restart, `recover`, a quarantined instance whose `dispose()` raises, control calls and Cleanup |
| Vendored console modules | [tools/ma3/gma3_mcp_hardkeys.lua](tools/ma3/gma3_mcp_hardkeys.lua) 0.10.0 (0.5.0 at the KB-07/KB-08 records), [gma3_mcp_feedback.lua](tools/ma3/gma3_mcp_feedback.lua) 0.2.0, [VENDOR.md](tools/ma3/VENDOR.md) (commits and hashes) | the MCP repository's harnesses (`npm test` there) |
| Surface service (Rust) | [service](service) — `auth` (framing, SipHash), `protocol`, `link` (pairing, seq/ev, retransmit, heartbeat, watchdog, state cache), `leds` (NX-K map, local fallback), `nxk` (decoder, USB via nusb), `sim`/`bench` | `cargo test` — 19 tests incl. a fake plugin exercising retransmission and loss, a release stopping its press's retransmission, nonce mismatch, old seqs, full/delta/epoch freshness, watchdog and re-pairing, `no-session` recovery, forged packets, LED diffs and the local fallback |
| The two together, no console | [tools/ma3/test/e2e.sh](tools/ma3/test/e2e.sh) | the real plugin under stock Lua with a stubbed console behind a UDP relay, driven by the real service: pairing, acks, a retransmission with zero losses, two refused keys, an abandoned session released by the lease, a 100-tap bench |

### Status

- **Verified end to end without a console** (`sh tools/ma3/test/e2e.sh`), see the
  table above (press-to-effect samples come from the stub's synthetic command
  line and say nothing about the console).
- **First run against onPC 2.5.1.0 on 2026-10-09**
  ([docs/probes/kb-07-live-macos-2.5.1.md](docs/probes/kb-07-live-macos-2.5.1.md),
  historical evidence): import and macro start work, 32 of 46 NX-K keys resolve
  on the default profile, a simulated tap reaches the command line, benches at
  2 and 10 taps/s lose nothing, the NX-K opens over nusb on macOS once the
  configuration is selected, and the operator confirmed the LEDs on the keypad
  (Record while Store pending, Clear while held, HighLight blinking, Bank and
  the encoder LEDs, the Link LED blinking when the plugin stops). Two findings
  changed the design during that day: HMAC-SHA256 → SipHash-2-4, and the
  module's shortcut-table re-read per event (7 ms) caps the plugin at roughly
  10 taps/s (KB-09). The record's "keypad row as a raw PC key" workaround was
  removed in the review below.
- **Qualified in KB-08** on the final revision: see below.
- Wheels are carried in the protocol and acknowledged `unsupported`: no
  verified Lua route for encoder input exists (separate work).
- Fade, Delay, Snap Shot and Back are not MA3 hardkeys; Load, Macro and Thru
  have no shortcut in the default profile and need one in the user profile
  (KB-09 part B plans an assisted way).

### Review of 2026-10-09 (five findings, all fixed)

1. A retransmitted press could execute after its release: event ids now order the events of a key
   (older than the newest processed → `superseded`, nothing dispatched), and the service stops
   retransmitting a press once it sends the release.
2. Unresolved releases were dropped across a restart: records are kept on stop, Cleanup and
   `service()` errors, adopted before input is enabled at the next start (key reserved, new press
   refused `conflict`), and released by the new `recover` command.
3. The keypad fallback bypassed route safeguards: removed; `+ - . /` now resolve through hardkeys
   0.5.0's `spec.prefer` with enablement, collision and route rechecks intact.
4. A retransmitted refusal was acknowledged as success: the original acknowledgment is cached per
   event id and replayed.
5. (second pass) A `dispose()` that raises no longer loses ownership: the instance is quarantined with
   its records, input is blocked at the next start, and `recover` exports and releases them before
   re-enabling the requested input mode.

Harness: 107 checks (was 87); service: 19 tests; `e2e.sh` passes. The shared module's preference change
is pushed as gma3-mcp `2d226dd` (PR #12, merged) and the vendored copy matches it byte for byte.

### Open defects found by KB-08 (owned here, not fixed in KB-08)

- **D1, bench instrument.** `bench` mode snapshots `cmdtext` after `hardkeys:press()` returns; a key the
  console applies within that call is invisible to the watch and the sample inherits the next tap's
  change. At 10 taps/s this produced a measured press-to-effect p99 of 156 ms (n=359) while an independent
  poller showed every tap landing. Fix the snapshot order, rerun with ≥ 360 samples, and report p99
  again; until then the p99 ≤ 120 ms limit is **not demonstrated**.
- **D2, flood handling.** The auth-failure throttle is per source address (a flood from the service's own
  address locks the service out for 10 s), and the 32-datagram read budget per iteration lets a 2000 pps
  flood from another address queue legitimate packets in the kernel past the retransmit budget (98 of
  195 events lost). The flood limit of section 8 is **not met**; decide between a time-bounded read
  budget that discards ignored sources cheaply, a deployment-specific limit, or a revised limit.

## KB-08 — Document and qualify supported deployments

**Request:** As a user, I want an accurate setup and compatibility statement for the delivered capabilities.

**Lua changes: none.** Documentation and qualification only; the two defects above are recorded
against KB-07. Caching (KB-09), automatic shortcut creation (KB-09 part B) and encoder support stay
separate.

**Delivered (2026-10-09):**

1. Documentation reconciled with the implementation: SipHash instead of HMAC, the raw-key workaround
   gone, 107 harness checks and 19 service tests, the protocol's retransmit parameters, section 8 with
   measured status, VENDOR.md with commits and hashes; the MCP repository's module pin, lock file,
   example version checks and compatibility matrix updated to hardkeys 0.5.0 and the surface
   evidence. The KB-07 probe record is kept unchanged as history.
2. The performance discrepancy resolved: the KB-07 table had used the 40-tap p95 (67 ms) as p99; the
   p99 was 183 ms. The rerun at the final revision with 359 samples measures 51 / 105 / 156 / 158 ms
   and is reported as not meeting the p99 limit, with the tail traced to defect D1
   ([docs/probes/kb-08-qualification-macos-2.5.1.md](docs/probes/kb-08-qualification-macos-2.5.1.md) §1).
3. Lifecycle observed on the console (same record, §2): clean service stop, cable pull (lease release
   1.99 s), plugin stop/restart with a physically held key (stale LED, re-pairing, no replay), user
   switch (feedback invalidated within 1 s), LED freshness (≈ 126 ms median), `status`/`recover`, the
   allow list. Exception and quarantine paths stay harness-tested and are labelled so.
4. A specific deployment matrix: [docs/deployments.md](docs/deployments.md) (MCP vs surface,
   same machine vs LAN, macOS/Windows/Linux, layouts, profiles, displays, hardware), with every
   untested combination named as unqualified.
5. An operator guide: [docs/operator-guide.md](docs/operator-guide.md) (install and update all three
   plugin components, pairing key, USB, supported and unmapped keys, exclusive input, status, stop,
   `recover`, what to do when something stays unresolved; `force` kept out of the normal start).
6. A reproducible acceptance record: [docs/kb-08-acceptance.md](docs/kb-08-acceptance.md) (commits,
   hashes, environments, commands, results, verdicts, exclusions; the real-service/stubbed-console
   e2e run beside the Lua and Rust suites).

**Closed** with the macOS beta scope of docs/deployments.md and the exclusions listed there and in the
acceptance record. Not closed as a pass on the p99 or flood limits.

## Follow-ups filed

- [bdstark/gma3-mcp#12](https://github.com/bdstark/gma3-mcp/pull/12) (merged): hardkeys 0.5.0 and the
  **KB-09** write-up: bounded per-iteration cache of the shortcut rows (lifts the ~10 taps/s ceiling), same-target tie
  resolution, and operator-managed profile shortcuts for Load, Macro and Thru.
- KB-07 defects D1 and D2 above.
- Unqualified deployments (Windows with WinUSB, a separate LAN host, a second display, non-US layouts,
  show reload with the plugin running) each need a recorded run before they are claimed.

## Next steps

1. Fix D1 in the plugin's bench mode, rerun `mtpnxk bench --taps 400 --rate 10` on the console and
   update section 8 and the acceptance record.
2. Decide D2 (read budget, throttle scope or limit) and re-measure the flood case.
3. With the NX-K attached, repeat the hands-on LED checks at the final revision and add them to the
   KB-08 record.
4. Windows qualification (WinUSB via Zadig) and a separate-machine LAN run when hardware allows.
5. Probe encoder input before routing wheels; keep them unsupported until a route is verified.

## KB-15 — Surface defaults, migration and qualification (surface half, 2026-10-09)

The module and bridge half is in the MCP repository (hardkeys 0.10.0, bridge 0.12.0, branch
`feat/kb15-mixed-backend`). This repository vendors 0.10.0 unchanged ([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md))
and adds the surface side in `mtpnxk_surface.lua` 0.2.0:

- `bank=<quickey>/<page>.<first>-<last>` provisions the owned Quickey bank from the plugin argument only
  (`authorized = true` is the operator's argument, never a surface request); codes default to the nine with
  KB-10 evidence (`bankcodes=hardkeys` for all 94). The record is kept across `stop` and adopted at the next
  start; `bank status | verify | teardown` are control calls on the running instance.
- `input=mixed` attaches `mixedBackend({ quickey = quickeyBackend(inst), keyboard = keyboardBackend(deps) })`
  with the policy `{ default = "quickkey", keys = <every key without KB-10 hold evidence -> shortcut> }`; the
  table is explicit (`NXK_QUICKKEY_HOLD`: NUM1, NUM5, THRU, PLEASE, CLEAR, STORE) because the surface holds every
  key, so a tap-only code (OOPS) is not a Quickey key. `input=quickey` is `{ default = "quickkey" }` alone.
  `route=<key>:<method>` overrides single keys; the module validates every method against the attached
  backend (`policy-unavailable`) and the surface refuses the start when an override resolves to no usable
  route. Without a bank the Quickey modes report the KB-12 requirement and press nothing.
- The start and `status` report the method, effective route and dispatching part per key (`describeRoute`);
  the welcome carries `backend`. `recover` attaches the part the kept records name (keyboard, quickey or
  mixed), as the bridge does.
- A stale run left by `ReloadAllPlugins` (state marked running, loop dead, socket still bound) is taken over
  by the next start instead of blocking it (found during this run).
- 0.2.1 (KB-16 review): a temporary shortcut-mode change that `dispose()` could not restore (KB-14: a dependent
  key still held, the restore delay not elapsed, or the profile/mode changed meanwhile) is no longer dropped at
  stop, Cleanup or quarantine export. The record is kept in `state.modeRecord`, reported by `status`, adopted at
  the next start as an unresolved restoration (every press refused `busy` until it is restored) and restored by
  `recover` on the original profile, exactly as the bridge does; harness: stop with a pending restoration,
  restart, refusal, status, recover, presses accepted again.

Harness: 143 checks (36 new: parsing, no-bank refusal, provisioning, the per-key report, Quickey executor
press/Unpress, both unqualified-mix directions, bank status/teardown refusals, record kept and adopted,
an unresolved Quickey record recovered through the quickey part, Quickeys-only refusals, policy refusals,
inert overrides, teardown and the no-fallback afterwards, stale-run takeover). The KB-14 mode operation
changed one KB-07 check: a key whose shortcut row is off is pressed with a temporary enable and restored.
`sh tools/ma3/test/e2e.sh` passes unchanged on the keyboard path.

**Verified live** (macOS, onPC 2.5.1.0, show `mcp-test-disposable`, the hardware service paired throughout):
[docs/probes/kb-15-surface-macos-2.5.1.md](docs/probes/kb-15-surface-macos-2.5.1.md). Not established: a
second display, show save/reload with the bank, Windows/Linux, LED behaviour seen by an operator, the
press-to-effect figure for Quickey taps (the bench's tap length dominates it, see the record).

## KB-16 — Qualify existing hardware protocols (surface half, 2026-10-10)

The Rust service gains `src/mtouch/`: control tables, report decoder and output encoders for the Martin
M-Touch (`11be:f808`) and M-Play (`11be:f80c`), ported from the MTouchPlay repository at `e48eb2c` and
regression-tested against its capture logs (17 tests; `cargo test` now runs 36), plus the operator commands
`mtouch-listen` and `mtouch-led-test`. What was reused, what was not re-verified, the regression table and
the live qualification procedure (recorded into `docs/probes/kb-16-hardware-<device>-<os>.md`) are in
[docs/mtouch-protocol-reuse.md](docs/mtouch-protocol-reuse.md). Both devices were qualified live on macOS the same day
([M-Touch](docs/probes/kb-16-hardware-mtouch-macos.md), [M-Play](docs/probes/kb-16-hardware-mplay-macos.md):
every control, report type and output write, idle polls, queued reports at open, unplug and replug), and nothing is wired
into the link (`run`/`sim`/`bench` remain NX-K only; integration is KB-20+). Hardware protocol
qualification does not establish grandMA3 behaviour: the console-semantics half of KB-16 lives in
bdstark/gma3-mcp `ENCODERS.md`.

## KB-17 — Shared control-context and binding snapshots (surface half, 2026-10-10)

The module half is in the MCP repository (`gma3_mcp_feedback` 0.3.0, bridge 0.13.0, branch `feat/kb17-context-snapshot`,
live record `docs/probes/kb-17-context-macos-2.5.1.md` there, 31/31). This repository vendors the 0.10.0/0.3.0 pair unchanged
([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md)) and adds the surface side in `mtpnxk_surface.lua` 0.3.0:

- the context items (data pool, executor page, encoder bank and slots of the configured display, one target per
  `execs=` executor) are watched next to the state items, so the loop's bounded reads keep them observed;
- every tick builds the module's cached `contextSnapshot()` and sends it as a `context` message
  ([docs/surface-protocol.md](docs/surface-protocol.md) section 1a) when its binding generation, `known` flag or epoch
  moved, and with the full-state cadence otherwise. No grandMA3 semantics are reconstructed here: unavailable parts
  carry the module's reason, and while a part is unobserved `known = 0` and no generation is claimed;
- the welcome carries `context: 1`; `status` counts `contexts`.
- the Rust service parses the message (`FromPlugin::Context`), keeps the last one as data (`Link.context`), counts
  received contexts and generation moves and logs each move with bank/page/context and the slot/executor counts.
  The context is dropped on link down, re-pairing and every new welcome, and `Link::context_view(now)` presents one
  only while paired with the link up, received in this pairing within `state_stale` and marked known by the plugin
  (`cargo test`: 38, two new).

Harness: 159 checks (8 new: the watch list, the message after the parts are observed with the missing encoder bar
explicit, cadence, an executor-page change moving the generation in the frame it is observed, back again, a show
change withdrawing the generation until every part is observed, resumption in the new epoch). `sh tools/ma3/test/e2e.sh`
unchanged on the keyboard path.

**Not verified live in this repository:** the surface plugin against onPC with the 0.3.0 pair (the MCP repository's
probe exercised the same module through the bridge); an encoder bar is only present on display 1 there, so the
plugin's default `display=1` is the one that answers. Wiring the context into encoder bindings is KB-18/KB-19.

## KB-18 — Continuous-control transport and admission (surface half, 2026-10-10)

The module half is in the MCP repository (`gma3_mcp_control` 0.1.0, bridge 0.14.0, branch `feat/kb18-control-admission`,
PR bdstark/gma3-mcp#22, live record `docs/probes/kb-18-control-macos-2.5.1.md` there, 30/30). This repository vendors the
0.10.0/0.3.0/0.1.0 set unchanged from `9f08f87` (both PR #22 review rounds included: late releases end their hold,
reserved release capacity, batched recovery, rebound holds on the generation change itself, stale snapshots refused,
binding revisions, releases kept across a rebind, the binding revision required unless a fixed binding is declared, which this plugin does) ([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md)) and adds the surface side:

- **protocol** ([docs/surface-protocol.md](docs/surface-protocol.md) section 3a): one `ctl` message for relative motion,
  absolute positions, touches and buttons, each with device, control, the service's per-device sequence `es`, the binding
  generation `cg` (the `context` message's), a gesture id and a target (`{slot}` or `{ex, el}`); the ack carries the
  module's outcome (`lost`, `coalesced`, `superseded`, the current `cg` on a stale refusal). Motion is never retransmitted
  (loss is reported by the plugin, never replayed); touches and buttons are retransmitted like keys and ordered per
  control. The welcome says `control: 1|0`.
- **`mtpnxk_surface.lua` 0.4.0:** the third vendored module is loaded with this plugin's cached context snapshot as the
  binding (so the service's `cg` and the module's generation are one number) and its hardkeys instance as the other
  input owner; a control session is opened, renewed, lapsed and reopened next to the hardkeys one; `ctl` packets are
  validated, deduplicated on `ev`, ordered per control for touches/buttons and handed to `control:submit()`; the loop
  services the module after feedback (4 intents per tick); lease lapse, `bye`, silence, `stop` and Cleanup end gestures
  through the backend and drop queued motion; a release the backend raised on is kept and adopted by the next
  `control=fake` start (`recover` re-attempts it); `status` prints the control line and its gestures. `control=fake|off`
  is a start argument, off by default (then `ctl` is acknowledged `control-disabled`); KB-18 ships the fake backend only.
- **Rust service** (`src/link.rs`, `cargo test` 45, nine new): the four rotaries are encoder slots 1–4 and a push is a
  button; Bank held is the `fine` modifier. `control_event()` keeps per-device sequences and per-control gestures,
  merges deltas per control/generation/gesture and positions only where the context reports a stateless fader
  function (review: a crossfade's or Temp's positions are all kept, bounded per control), sends at most one motion packet per control per 5 ms
  (16 per tick) and drops queued motion older than 250 ms, against a moved generation or when the link goes down; a
  boundary flushes its control's motion first, is retransmitted, and a release supersedes an unacknowledged press.
  Without a known context motion and downs are dropped (`ctl_unbound`); a plugin reporting `control: 0` gets none
  (`ctl_unsupported`). The summary line prints the control counters; `sim` accepts `rot<n>:<delta>` and `btn<n>:down|up|tap`.

Harness: 187 checks (26 new: default off and `control-disabled` acks, `control=fake` and the welcome flag, malformed `ctl`
packets rejected before the module, stale generation with the current one in the ack, admission and coalescing with the
acks saying so, duplicate ids, loss reporting, out-of-order, the fake backend's recorded intents, touch/button order,
positions superseding, the busy descriptor, a generation change refusing old-generation motion, a second surface's
conflict, lease lapse and `bye` ending gestures through the backend, the legacy `wheel` answer, `status`, a kept unresolved
release adopted and recovered). `sh tools/ma3/test/e2e.sh` gains a control scenario: the real service's rotary deltas and
push reach the real plugin as `ctl` events and are refused `target-unavailable` (the stub console has no encoder bar), with
nothing lost and the counters as expected.

**Not verified live in this repository (at KB-18):** the surface plugin and the NX-K rotaries against onPC with `control=fake` (the
MCP repository's probe exercised the same module through the bridge, 30/30); the M-Touch strips are not wired into the link
(touch and absolute events are exercised by the Rust and Lua harnesses only; KB-20). Nothing moves on the console in
KB-18: the adjustment backend, calibration and the meaning of `fine` are KB-19 (below).

## KB-19 — NX-K encoders follow the active encoder context (surface half, 2026-10-10)

The console half is in the MCP repository (`gma3_mcp_control` 0.2.0 with the **console adjustment backend**,
`gma3_mcp_feedback` 0.4.0 with each slot's physical range, bridge 0.15.0 `control=console`, branch
`feat/kb19-adjustment-backend`, PR bdstark/gma3-mcp#23, live record `docs/probes/kb-19-adjust-macos-2.5.1.md` there,
39/39). This repository vendors the 0.10.0/0.4.0/0.2.0 set unchanged from `107f548` ([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md))
and adds the surface side:

- **Slots, not attribute names:** rotary n is encoder slot n of the bound display (section 3a); the vendored backend
  applies `Attribute "<name>" At +/- <detents x step>` for whatever the plugin's own context snapshot says slot n holds
  (name, layer, resolution, readout, channel function, physical range). The surface never names an attribute.
- **Calibration:** one NX-K detent is one console encoder click: Percent/PercentFine readouts 1 at Coarse, Physical
  readouts (Pan, Tilt, gobo and colour wheels) the attribute's range / 120 in physical units, Fine a tenth (the manual's
  encoder-resolution rule, measured live in the MCP repository: Dimmer `At + 1`, Pan `At + 15` for four detents).
  **No acceleration** is applied in the service or the plugin: the NX-K's own delta per report is the detent count (the
  summary line's `max_detent` shows the largest single-event delta the device reported, so device-side acceleration, if
  any firmware does it, is visible rather than compounded).
- **Fine gesture:** Bank held sends `fine`; the backend divides the click by 10. It is an explicitly smaller step, not
  the console's own resolution toggle.
- **Pushes separate from rotation:** a rotary push is a `button` event and the console backend refuses it `unsupported`
  at admission (calculator / open / select is not qualified; nothing is pressed, nothing owned). Strip touches and
  positions are refused the same way (KB-20).
- **Unavailable slots:** the plugin acknowledges `target-unavailable` with the binding's reason (no selection,
  unavailable for the selection, empty slot) or `unsupported` (phaser/editor slot, unqualified readout/resolution/layer/
  channel function, unreadable range); the service counts them (`refused`) and prints the reason with `--verbose`.
  A `mixed` slot (some selected fixtures lack the attribute) is admitted: the console moves the fixtures that have it.
- **Fifth slot / outer ring:** `--rotary-slots <base>` (1 by default) is the explicit, documented window: `2` maps the
  rotaries to slots 2-5, so a page's fifth pool slot is reachable; the outer ring (dual-encoder function) is reported by
  the context and refused, never silently mapped. Nothing is discarded silently.
- **Context changes:** queued motion against a moved generation is dropped by the service (250 ms / generation rules of
  KB-18) and by the plugin's loop; the next fresh gesture uses the new binding.
- **`mtpnxk_surface.lua` 0.5.0:** `control=console` next to `control=fake|off` attaches the vendored
  `consoleBackend(consoleDeps(_G))` (`Cmd` is the only console API it uses); `recover` attaches the backend of the
  configured mode; the welcome says `controlBackend: console`; `status` names it.
- **Rust service** (`cargo test` 46): `--rotary-slots`, the slot-window mapping and the `max_detent` counter.

Harness: `lua tools/ma3/test/surface_plugin_test.lua` 195 checks (8 new: `control=console` enabling the vendored backend,
the welcome flag, two detents applied as `Attribute "Dimmer" At + 2` through the stub console's `Cmd`, three fine
detents `At - 0.3`, four detents on a Physical slot `Attribute "Pan" At + 15`, a push and a strip position refused
`unsupported`, `status`); `cargo test` 46; `sh tools/ma3/test/e2e.sh` unchanged (the stub console has no encoder bar).

**Verified live in this repository:** see [docs/probes/kb-19-surface-macos-2.5.1.md](docs/probes/kb-19-surface-macos-2.5.1.md)
(the surface plugin on onPC with `control=console`, driven by the real service's `sim` rotary script, the console's
programmer value read back through the MCP bridge).

**Not verified live:** the physical NX-K rotaries (the `sim` path exercises the same service code from the decoder
onward; the operator qualification with the device in hand is KB-24's), two surfaces on one slot, Windows/Linux hosts,
M-Touch strips (KB-20).

## KB-20 — M-Touch parameter strips act as encoders (surface half, 2026-10-10)

The console half is in the MCP repository (`gma3_mcp_control` 0.3.0: a strip **touch** on an attribute slot is a hold
the console backend serves, a strip **position** is `Attribute "<name>" At <value>` over the verified travel, mixed
values are refused unless the event says takeover; bridge 0.16.0; branch `feat/kb20-strip-adjust`, live record
`docs/probes/kb-20-strips-macos-2.5.1.md` there). This repository vendors the 0.10.0 / 0.4.0 / 0.3.0 set unchanged from
`01e1561` ([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md)) and owns the gesture conversion (`service/src/strips.rs`):

- **Which strips:** the M-Touch's four right-hand base-channel faders (`VALUE BASE CHANNEL 1..4`, 0x6112/0x6122/0x6132/
  0x6142: 8-bit position 0..255 bottom to top, a `touch` flag, a lift report with the resting value; KB-16 record) are
  `Strip1..Strip4`, encoder slots `--strip-slots`..+3 of the bound display (1-4 by default, `2` reaches the fifth pool
  slot, as `--rotary-slots`). The ten playback strips are not parameters (KB-21/22); the pressure keys are never input.
- **Relative touch-drag by default (`--strips relative`):** the first report with the finger down **anchors** the strip
  and sends the touch down (no jump to the touched position); every later report moves the slot by `(counts moved) x
  120 / 255` detents with the fraction carried exactly (integer arithmetic: a full stroke is exactly 120 detents, the
  five encoder turns of one attribute range; `--strip-travel` changes it); the lift sends the touch up and nothing
  else. One count of jitter (0.47 detents) never emits on its own. Lifting and retouching re-anchors: repeated travel
  across a large range is lift, touch, drag again.
- **Direction, sensitivity, fine as the rotaries:** up is positive; the detents reach the console as the same `rel`
  events the NX-K produces (one detent = one console click, KB-19 calibration); `fine` is the NX-K's Bank held **or**
  the strip's own capacitive key (`BASE CHANNEL n`, 0x61n1) held.
- **Binding changes while touched:** a moved context generation marks the touch rebound: its motion is dropped in the
  service until the lift (the plugin would refuse it `gesture-rebound` anyway); the next touch is a fresh gesture against
  the new binding. `--verbose` prints "lift and touch again".
- **Missed touch-up, reconnect:** a touch longer than 30 s without a lift report is ended locally (the console's own
  `maxGestureMs` is the same bound); the next report with the finger down re-anchors. The M-Touch is optional and
  hot-pluggable in `run` (`--mtouch auto|off`): opened when present, retried every 2 s while absent, and when it goes
  away every touched strip is released; reports queued on the device before the open are discarded, never used to seed
  a touch. The keypad going away releases the strips before the devices are reopened.
- **Absolute mode (`--strips absolute`, an explicit choice):** positions (`abs`, 0..1 of the travel) go out only after
  **pickup** (the strip crossed, or landed within 0.02 of, the slot's last known value from the plugin's context: `abs`
  as a percentage of the range, the same for every readout) or an **explicit takeover** (the strip's key held at
  touch-down, which marks the events `tk: 1` so the console backend places the value on a mixed selection). A slot whose
  value is unknown, empty or mixed cannot be picked up: the strip waits (`waiting_pickup` in the summary) until a
  takeover. Reconnects, page changes and touch-down therefore never jump; a page change while touched is a rebound. The
  console backend places the value over the verified travel only (Percent 0..100, Physical From..To) and refuses a
  mixed physical range. The context's slot values refresh on a generation change and at least once a second, so pickup
  is against the binding's last read, not a live value (documented limitation).
- **Mixed values stay mixed:** in relative mode nothing collapses them; in absolute mode the plugin's vendored module
  refuses a position on such a slot `mixed-values` unless `tk: 1` was sent, which only the strip's key produces.
  **Incomplete reads are not agreement** (review of PR #5 / MCP PR #24): the context's `vc` is 1 only when feedback
  read every selected fixture (`valueComplete`); a `val: "value"` from a bounded scan (beyond the module's eight-fixture
  scan bound) or a failed programmer read carries `vc: 0` and is never a pickup target here, and the vendored module
  refuses a position on it `values-incomplete` unless `tk: 1`. A context without `vc` (an older plugin) is never
  assumed complete.
- **Devices are independent** (review of PR #5): `run` opens the NX-K and the M-Touch separately and retries each
  every 2 s while it is absent; an M-Touch alone polls its strips, losing the NX-K does not interrupt the M-Touch
  (LED writes go nowhere until it is back, then every LED is repainted), losing the M-Touch releases every touched
  strip and the device is reopened. **A lost NX-K releases what it held** (review round 2): the loop releases every
  key the link holds for it (a pending press is superseded by its release, as for a physical release, so heartbeats
  stop reporting it), ends every rotary push and clears the Bank modifier, so the strips go back to coarse; a finger
  on a strip is untouched by the keypad's loss. `service/src/usb_surfaces.rs` is unit-tested with fake devices
  (M-Touch-only startup, NX-K removal during M-Touch operation, M-Touch loss and reopen) and the loop with a scripted
  surface (Clear, Bank and a rotary push held, the keypad lost: held set empty, push ended, the next strip move no
  longer fine, the touched strip still touched).
- **M-Play:** its faders are playback faders (KB-21/22); none is mapped as a parameter strip here.
- **`mtpnxk_surface.lua` 0.6.0:** carries `tk` (a position's takeover flag; `bad-tk` elsewhere) to the vendored module;
  the console-backend log line names the strips.
- **Rust service** (`cargo test` 60, fourteen new): `strips.rs` (anchor, carried fraction, full stroke, jitter, fine
  from Bank or the strip key, the slot window, rebound, missed lift, disconnect, off, absolute pickup/takeover), `sim`
  strip steps (`strip<n>:t<v>|m<v>|lift|l<v>`, `skey<n>:down|up|tap`) and `poll_strips`, `tk` on the wire, the
  `--strips/--strip-slots/--strip-travel/--mtouch` flags, a `strips:` summary line, the pickup hint requiring `vc`
  and the independent device slots of `usb_surfaces.rs`, and the keypad-loss release in the loop.

Harness: `lua tools/ma3/test/surface_plugin_test.lua` 205 checks (10 new: a strip touch as a hold, the drag inside it
`At + 3`, a position `At 25`, a Physical position `At -180`, `tk` carried, bad `tk` rejected, the lift as a boundary,
`vc` in the context, a bounded-scan value refused `values-incomplete` with `vc` 0); `cargo test` 60; `sh tools/ma3/test/e2e.sh` adds the strip scenario (touch, drag and lift refused honestly by the stub
console without an encoder bar, the lift a noop).

**Verified live in this repository:** [docs/probes/kb-20-surface-macos-2.5.1.md](docs/probes/kb-20-surface-macos-2.5.1.md)
(the surface plugin on onPC with `control=console`, driven by the real service's `sim` strip scripts: relative
touch-drag, re-anchoring retouch and the strip key as fine read 40 -> 49 -> 40 -> 40.9 on the console's programmer;
absolute mode waited for pickup against the context's 40.9 and placed `At 43.1373` and `At 54.902`).

**Not verified live:** the physical M-Touch strips through `run` (the `sim` path enters the service after the decoder;
the hardware qualification with the operator at the surface is KB-24's), an explicit takeover on a mixed selection
through this plugin (the MCP repository's probe covers it through the bridge), two surfaces on one slot,
Windows/Linux hosts.

## KB-21 — Playback banks and stable executor bindings (surface half, 2026-10-10)

The console half is in the MCP repository (`gma3_mcp_feedback` 0.5.0: an executor target is explicit, pool / page /
executor / element / mode, read on the user's page or on an explicit page through `ObjectList("Page P.E")`, a missing
page is `pageMissing` and never created, a wider neighbour of the same row covers the following numbers; `gma3_mcp_control`
0.4.0: targets resolve by (page, executor), a hold's release goes to the target its down resolved, frozen; bridge 0.17.0;
branch `feat/kb21-playback-banks`, live record `docs/probes/kb-21-executors-macos-2.5.1.md` there). This repository
vendors the 0.10.0 / 0.5.0 / 0.4.0 set unchanged from `d60bb06` ([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md)) and owns
the banks (`service/src/banks.rs`): which physical control means which executor number is a profile decision made in
the service, never inferred from the console.

- **Executor page vs surface bank:** the page is the console's; a bank is a window of this surface's controls on it.
  Bank `k` (0-based) of a section of `size` strips maps strip `i` to executor `row + k x size + (i - 1)` on each of the
  section's rows (`--pb-rows <keyA>,<fader>,<keyB>`, default `101,201,301`: the console's default layout of a button
  above, the fader's own executor and a button below; the M-Play's right block is one row, `--pb-ext-row`, default 401).
  A bank never leaves its row's hundred block (the M-Touch's ten strips: banks 1-9 = 101-190; the M-Play's twelve: 1-7;
  its 24-button block: 1-3); the bank keys stop at the edge, counted (`edge`) and logged.
- **Sections and keys:** the M-Touch's ten playback strips (`PFA n` 0x42n1 and `PFB n` 0x42n2 are the keyA / keyB rows'
  buttons; its `PLAYBACK BANK +/-` 0x4111 / 0x4113 move its bank) and the M-Play's two sections (left: twelve strips with
  a top and a bottom button, `FADER BANK` 0x9F12 / 0x9F13; right: 24 `FADER EXT PF` buttons, `FADER EXT BANK` 0x9E12 /
  0x9E13), each banked on its own. The service opens the M-Play next to the M-Touch (`--mplay auto|off`, reopened every
  2 s like the others); only an **attached** device's windows are bound. Command keys, MF keys and the pressure keys stay
  unmapped (counted). The playback **faders** are not operated here (KB-22: pickup against the executor's level and the
  configured fader function); their reports are counted `ignored_faders`.
- **Follow or independent:** `--pb-page follow` (default) binds the window's executors without a page: the plugin reads
  them on the console's current page, a `Page n` on the console moves the binding generation and the targets are the new
  page's. `--pb-page <n>` binds them to that page: read whatever page the console shows, the generation ignores the
  console's page changes, a page that does not exist is `pageMissing` per target (`pm: 1`) and nothing creates it. Local
  banking only replaces the bound list; the service never issues a page command.
- **The binding goes over the wire:** a `bind { ex[], pg? }` packet (retransmitted until acknowledged, sent again after
  every pairing, on every bank change and when a device appears or goes away); the plugin re-watches exactly that list
  (bounded, 96: the M-Touch's 30 and the M-Play's 60 fit together on any bank; more is refused `bad-bind` and the previous
  binding kept), the vendored control module turns the replaced spec into a new **binding revision** and the ack carries
  it (`br`). The context message carries `bind { ex, pg, br, mode }` and each executor's `pg` (bound page), `pgno` (the
  page read), `w` (width), `cov` (a covering neighbour), `pm`; motion and downs carry `br`, so an event produced against
  the old window is refused `stale-binding`, never resolved against the new one.
- **Frozen holds:** a pressed button's target is recorded at the down and the release carries it, whatever bank the
  section is on by then (`frozen` counted and logged: "released executor 101 key (pressed before the bank moved; now
  111)"); the plugin freezes the resolved target on its side as well (status shows `frozen=`). A bank change while a
  button is held therefore neither releases nor activates the newly mapped executor; a fresh press reaches it. A device
  that goes away releases what it held against the frozen targets.
- **Strip gestures on rebinding:** a new binding revision (our bank change applied, or the plugin rebound on its own) marks
  every touched parameter strip rebound exactly as a moved generation does (`rebinds` counted; "lift and touch again"),
  so absolute mode picks up or takes over again on the next touch.
- **Indicators and status:** the M-Touch page display (0x4401) and the M-Play's displays (0x9F01 left, 0x9E01 right) show
  "page then bank" as `page x 10 + bank` (page 1 bank 2 = `12`; the console's page in follow mode, the fixed one
  otherwise) while both fit, the bank alone otherwise, blank while the page is unknown; written on change only. The
  service prints the profile at start and a `banks:` summary line after `sim`/`bench` (mode, page, each section's bank
  and window or `absent`, held, changes, edge, downs, ups, frozen, ignored_faders, unmapped); `--verbose` logs every bank
  change, frozen release, bind and revision. The plugin's `status` prints `binding: N executor(s) [...] following the
  console's page | on page P (independent) revision=R generation=G` and each held gesture's frozen target.
- **Layouts are the console's:** an expanded assignment (width > 1) covering its neighbours, an empty, reserved or
  Quickey executor and a missing page are reported by the plugin and refused per target with the reason
  (`target-unavailable`); nothing here assumes adjacent numbers are separate playbacks.
- **Review of PR #6:** the watch list is built through the feedback instance (its bound of 96 executors) with `maxItems`
  budgeted for every supported configuration at once (9 state readers + two per `execs=` entry, itself bounded at 96, + 4
  context items + 96 targets = 301), so an M-Play alone (60), both surfaces on different banks (84) and `execs=201..220` with
  a bind of 90 are watched in full and the context becomes known; a bind whose list the feedback could not watch in full is
  refused `bad-bind` and the previous binding re-watched (round 2); losing the
  M-Touch releases its held playback buttons against their frozen targets (the M-Play's holds stay); the vendored control
  module carries the frozen record through the gesture-bound force-end, a raised release, `recover()` and adoption.
- **`mtpnxk_surface.lua` 0.7.0:** the `bind` handler (validation, the bound list, `rewatch`, the revision in the ack), `pg`
  on executor targets and `br` on events, `bind`/`pg`/`pgno`/`w`/`cov`/`pm` in the context, the binding line in `status`;
  the KB-06 per-executor state items (`x<n>`, `f<n>`) follow the `execs=` argument only; the feedback instance is configured
  for 96 executors and 8 reads per tick.
- **Rust service** (`cargo test` 73, thirteen new): `banks.rs` (ids, the default profile and its banks, row edges, the
  M-Play sections, independent page, frozen holds across a bank change, a lost device, present devices only, displays and
  the summary, unmapped keys), `link.rs` (the bind after pairing and re-pairing, retransmission, its ack and refusal, `br`
  on motion and downs, the revision reported once, page-bound targets on the wire, supersession by (page, executor)),
  `sim` steps `pfa<n>|pfb<n>|mpa<n>|mpb<n>|mpe<n>:down|up|tap`, `pbank|lbank|rbank:up|down`, `pb<n>:t<v>|m<v>|lift`, the
  `--pb-page/--pb-rows/--pb-ext-row/--mplay/--banks` flags, `device_present`/`poll_mplay`/`output` on the surface trait.

Harness: `lua tools/ma3/test/surface_plugin_test.lua` 226 checks (21 new: a bind of 60 and of 84 executors watched in full and known, `execs=201..220` plus a bind of 90 (142 items) known, a truncated watch list refused with the previous binding kept, `execs=` bounded and validated, `execs=` following the page, a bind
acknowledged with its revision (sorted, deduplicated, the page kept), the watch list, the context's `bind` and per-executor
page, a missing page accepted and reported `pm`, malformed and oversized binds refused, a following bind again, a down frozen
to page 3's object, the rebind moving the revision and marking the hold rebound with the frozen target unchanged, the release
applied to the frozen target while the number means another object on another page, an old revision refused
`stale-binding`, a fresh down reaching the new mapping, a page-bound down admitted and one without `pg` refused, bad `pg`/`br`
rejected, the status line); `cargo test` 73 (a lost M-Touch releases its playback buttons, not the M-Play's); `sh tools/ma3/test/e2e.sh` adds the bank scenario (a simulated M-Touch alone
binds 30 executors; `PLAYBACK BANK +` moves to bank 2 and the display shows `12`; a `PFA 1` press on bank 2 is refused
`executor 111 is empty` by the stub console, its release after `PLAYBACK BANK -` goes to the frozen 111 (`frozen=1`), the
display is back at `11`; two playback fader reports are counted, not sent).

**Verified live in this repository:** [docs/probes/kb-21-surface-macos-2.5.1.md](docs/probes/kb-21-surface-macos-2.5.1.md)
(the surface plugin on onPC with `control=fake`, driven by the real service's `sim` scripts: a bank change while `PFA 1` was
held released page 1's executor 201, not the newly mapped 211; a tap on bank 2 refused `executor 211 is empty`; an
independent page 2 refused `executor 201 of page 2 is empty` while the console stayed on page 1; a console `Page 2` / `Page 1`
moved the following binding's generation once each; the page display read `11` / `12` / `21`).

**Not verified live:** the physical M-Touch and M-Play through `run` (the `sim` path enters the service after the decoder;
the hardware qualification with the operator at the surfaces is KB-24's), the executor operations themselves (KB-22), an
expanded assignment on the console (the MCP record says what the console answered), Windows/Linux hosts.

## KB-22 — Playback faders and buttons follow executor configuration (surface half, 2026-10-10)

The console half is in the MCP repository (`gma3_mcp_control` 0.5.0: the console backend serves an executor key as
`Press`/`Unpress Page <p>.<e>`, the console's own dispatch of the CONFIGURED button function, for the key functions
qualified live (Temp, Flash, Toggle, Top, Go+; any other configured function, or an empty one, is refused `unsupported`
naming it), an executor fader as `Fader<Function> Page <p>.<e> At <level>` for the CONFIGURED fader function, qualified
separately per function (Master and Temp; Rate, Speed, X/XA/XB, CrossFade and Time refused until qualified, with their
travel and neutral position recorded in `FADER_FUNCTIONS`), a fader touch as a hold, executor encoders refused; a release
whose executor was reassigned or emptied while held is kept as an `assignment-changed` record instead of operating the
replacement, re-attempted by `recover()`; bridge 0.18.0; branch `feat/kb22-executor-operations`, probe
`scripts/kb22-probe.mjs`). This repository vendors control 0.5.0 from `0315e18` ([tools/ma3/VENDOR.md](tools/ma3/VENDOR.md))
and owns the playback faders, the pressure buttons, the pickup display and the dedicated-key profile (`service/src/banks.rs`):

- **Configured action, separate down/up:** nothing changes on the wire for buttons: a strip's upper and lower buttons
  go out as `btn` down/up on the executor's `key` element and the console runs the executor's own KeyPress/KeyUnpress.
  Each physical strip button maps to one explicit element (`--pb-rows`); no button is invented on an executor.
- **Playback faders operate the fader row's executor** through its `fader` element: the touch-down is a hold on the
  executor the strip means right then (frozen, like a button: a bank change while touched neither moves the hold nor
  reaches the new mapping; the lift ends it), positions go out as `abs` 0..1 of the strip and the console backend scales
  them over the configured function's travel. The service never assumes a function: an unqualified one is refused by
  the plugin and logged.
- **Pickup by default:** a position goes out only after the strip crossed the executor's last known level (the
  context's `lvl`, the configured fader function's level read by the plugin, refreshed on a generation change and at
  least once a second) or landed within `--pb-pickup` (0.02 of the travel) of it. There is no takeover modifier for
  playback faders. An executor whose level is unknown (not a playback target, no fader function, no context) is never
  picked up: the strip waits, counted `waiting`.
- **Pickup resets** on every touch, after a bank change or a plugin rebind (the touch is rebound: positions dropped until
  the lift, counted `rebound`), after a reconnect (a lost device lifts its faders against their frozen targets, counted
  `forced`) and after an **external edit**: a context received more than `external_grace` (1.2 s, one refresh) after our
  last position that reports the level more than twice the tolerance away from it means someone moved the fader on the
  console; the strip must cross the new level again (counted `resets`, logged "edited elsewhere"). Our own position's
  lag in the context (a refresh within the grace) never resets it.
- **Never from the device:** a report with `touch` 0 while nothing is touched (the device's startup or resting reports;
  the first 250 ms after an open are discarded anyway) seeds nothing: no level is ever initialised from the surface.
- **Pickup direction on the bar:** while a touched fader waits for pickup, its RGB bar (M-Touch 0x42n3, M-Play 0x92n3)
  shows the span between the strip's position and the executor's level, yellow and blinking; cleared on pickup, lift,
  rebind and loss; written on change only. Level, activity and assignment colours are KB-23's.
- **M-Play buttons are pressure keys:** the hardware reports `FADER PFD 1-24` and `FADER EXT PF 1-24` as analog
  pressure units (kind V; the KB-16 record and the decoder), which the loop used to count unmapped: the first report
  with `touch` is the down, the release report the up, the pressure value is never an input (`banks.pressure`). The
  M-Play's twelve faders are routed by id (`playback_fader_of`), no longer counted blindly. The M-Touch is opened for
  its playback strips even with `--strips off`.
- **Assignment changed while held:** the frozen record goes to the plugin unchanged; the vendored module keeps the
  release as an `assignment-changed` record (nothing is issued on the replacement), the plugin's `status` prints it with
  the KB-22 note and `recover` releases it once the original object is back on that executor (or the operator ends the
  playback by hand).
- **Page / bank navigation:** unchanged from KB-21: a bank change only replaces the bound list; the console's page
  commands are never issued here; the MCP probe checks live that `Page n` neither stops a running playback nor starts
  the newly visible ones.
- **Dedicated keys through an explicit profile only:** `--pb-go <executor>` makes the `Next (Go)` key (0x5513, the same
  id on both devices) press that executor's `key` element (its configured function, whatever it is: nothing here assumes
  Go+), `--pb-pause <executor>` the same for `Pause (||/Back)` (0x5512); the executor is on the bound page, never banked,
  and is bound with the windows. Without the flags the keys are unmapped and counted; SELECT, RELEASE, BEAT and SNAP are
  ambiguous and stay unmapped.
- **The rebind race (found by the e2e scenario):** events carry the binding revision from the plugin's context, which
  was refreshed only at the next periodic send after a bind, so a press within a second of a bank change was refused
  `stale-binding`. The plugin now pushes the context on the tick after a bind, and the service adopts the revision from
  the bind ack at once (`current_binding` takes the newer of the two; held gestures are rebound then).
- **`mtpnxk_surface.lua` 0.8.0:** the context push after a bind, the KB-22 wording of `control=console`, the
  assignment-changed note in `status`; vendored control 0.5.0 (feedback 0.5.0 and hardkeys 0.10.0 unchanged).
- **Rust service** (`cargo test` 80, seven new in `banks.rs`): the fader hold and pickup (crossing, tolerance, a resting
  report seeds nothing), an unknown level never picks up, bank/generation rebinds and the fresh touch, the external-edit
  reset against our own lag, a lost device and a missed lift, the M-Play pressure buttons and fader ids, the pickup
  bar's span, the dedicated keys' profile and binding; `--pb-pickup`, `--pb-go`, `--pb-pause`; `sim` steps
  `mpf<n>:t<v>|m<v>|lift`, `go|pause:down|up|tap`, M-Play buttons simulated as pressure reports; `banks:` summary
  extended (`faders: touched touches lifts positions pickups waiting resets rebound forced dedicated`).

Harness: `lua tools/ma3/test/surface_plugin_test.lua` 235 checks (9 new, against a stub Sequence assigned on page 1 with
`control=console`: the bind acked with its revision and the context pushed on the same tick, the context listing the
executor's functions/level/activity, a button down applied as `Press Page 1.201`, a fader touch and position applied as
`FaderMaster Page 1.201 At 50`, a reassignment while held moving the generation, the release admitted but not issued,
`status` naming the assignment-changed record, `recover` keeping it until the original object is back and then issuing
`Unpress Page 1.201`); `sh tools/ma3/test/e2e.sh` extends the bank scenario (a fader touch travels as a hold on executor
201's fader, refused empty by the stub console; the move waits for pickup; an M-Play pressure button is one down and
one up, refused empty; the Go key without a profile is unmapped; the summary's fader counters).

- **Review of PR #7 (round 1):** (6) after a bind ack the link adopts the revision but reports the generation as
  **unknown** until a context of that revision arrives (`current_generation`): no down or motion combines the new `br`
  with the old context's `cg` (a new binding can reuse the old generation number); releases still go out; the plugin's
  context push after a bind keeps the gap to one tick. (7) The external-edit detection no longer restarts with every
  position: each context is judged once against the positions in effect when the plugin could have read it (the last
  1.2 s before its arrival and the one before that window); a match confirms our level, no match for longer than the
  grace since the first unconfirmed position resets the pickup, so continuous reports can neither hide an edit nor
  cause a false reset while the console follows with its refresh lag. The vendored control 0.5.0 from `3e0e7c4` adds the
  console-side rules (a hold tracked until its release applied, queued downs revalidated, frozen releases on a current
  observation in the same data pool through this plugin's `deps.observe`, same-session conflicts). `cargo test` 82
  (two new), harness 235, e2e unchanged. **Round 2** (control from `ce91371`): the verification is the live `deps.observe`
  read only (never the cached binding), a hold whose down never applied is discarded at the session's end without a
  release, and an unresolved release reserves its target against new presses until `recover` applies it; the harness
  check that raises on a cleanup release now services the touch first (an unapplied hold is discarded, by design). **Round 3** (control from `d77888d`): the gesture bound cancels an expired gesture's queued intents before
  the queue is serviced; reservations compare the executor's stable address (pool, page, number, element), so a changed
  key function or a reassignment cannot open an executor with a release pending on it.

**Verified live in this repository:** [docs/probes/kb-22-surface-macos-2.5.1.md](docs/probes/kb-22-surface-macos-2.5.1.md)
(the plugin 0.8.0 on onPC with `control=console`, driven by the release service's `sim` with `--pb-rows 201,201,301`: a
PFA tap and two Go-profile taps ran `Press`/`Unpress Page 1.201` and `1.195`; a fader below an unreachable level sent
nothing; an M-Play pressure button was one down and one up and its fader picked up at the level and placed
`FaderMaster Page 1.201 At 98.04`; a bank change rebound the touched fader and the lift went to the frozen executor; an
external `FaderTemp ... At 30` while PB10 was held reset the pickup ("edited elsewhere") and no further position went out;
a touch above 210's Temp level waited, crossing 0 picked up and `At 0`, `At 39.22`, `At 0` started and ended the Temp
playback; no refusals, nothing unresolved).

**Not verified live:** the physical M-Touch and M-Play through `run` (KB-24's hardware qualification), the bar display
on the device, Rate/Speed/crossfade fader functions (unqualified on the console half), Windows/Linux hosts.

## KB-23 — Parameter colors and playback feedback (surface half, 2026-10-10)

The console half is in the MCP repository (`gma3_mcp_feedback` 0.6.0: every attribute slot carries the feature group /
feature / activation group names parsed from the definition's own path text and the definition's `Color` as numbers
with `colorDefined` (a black colour, which Dimmer and Gobo have live, defines none), every assigned executor's
appearance carries its `BackRGBA` as 0..1 components; colour and appearance stay out of the binding digest; the bridge
0.18.0 and control 0.5.0 are unchanged; branch `feat/kb23-parameter-colors`, probe `scripts/kb23-probe.mjs`). This
repository vendors feedback 0.6.0 from `9535ded` (PR #27 review round 1: numeric-only hex parses as hex; [tools/ma3/VENDOR.md](tools/ma3/VENDOR.md)) and owns every LED
translation (`service/src/lights.rs`):

- **Colours from verified metadata, never from a label:** the plugin's context carries per attribute slot `fg`, `fn`,
  `ag` (feature group, feature, activation group) and `col` (the definition's colour, six hex digits, only when the
  show defines one) and per executor `col` (the appearance background). The service's `param_colour` takes the
  definition's colour first, then a documented override table on the attribute's **system name** (`ColorRGB_UV` ->
  violet shown as magenta, `_RY` amber -> yellow, `_W` -> white, `_R/_G/_B`, `_C/_M/_Y`), then the feature-group
  category scheme for non-colour parameters (Dimmer white, Position cyan, Gobo magenta, Beam yellow, Focus cyan,
  Control red, Shapers blue, Video green), else a neutral blue. The band label (`label`, translated) is textual only.
  These colours identify the parameter; nothing claims the fixture's emitted colour or UV light.
- **The M-Touch's parameter strips are blue-only hardware** (key 0x61n1, bar 0x61n2; KB-16 record, the
  `mtouch-protocol-reuse` tables): the parameter colour cannot be shown on them. It is carried textually (the `light:`
  lines of `sim`, the `--verbose` `lights:` lines) and the scheme renders on RGB-capable bars, which none of the default
  layouts puts a parameter on (KB-24's optional M-Play parameter strips would). The blue bar shows the **value**:
  n of ten LEDs for a single, completely read value (`val` value, `vc` 1; at least one LED above zero, never ten below
  full); a blinking level for an incompletely read value (`vc` 0: the readable fixtures' agreement only); the two END
  LEDs blinking for **mixed** values (the context's `abs` holds the first fixture's value there, and it is never shown
  as a level); dark for empty, none, unavailable and no selection. The strip's key is lit while the slot is adjustable
  (`avail` available or mixed) and fast-blinks while touched.
- **Playback feedback keeps assignment, running, pressed, pickup and stale apart:** the blue PFA/PFB keys (M-Touch,
  single colour) show assigned (on), running (slow blink), pressed (fast blink); the red/green pressure keys (the
  M-Touch PFD of the fader row, every M-Play button) show assigned red, running green, pressed fast-blinking in the
  current colour; the RGB bar shows the configured fader function's **level** (`lvl`) in the **assignment colour**
  (the appearance background reduced to the bar's 3-bit palette: a lane counts when it carries at least half of the
  brightest one, so orange `FF7F00` is yellow; too dark or no appearance -> neutral green). The KB-22 pickup span
  (yellow, blinking) keeps the bar while a touched fader waits; the renderer's cache follows that write and repaints the
  level once the span is cleared.
- **Stale is a distinct unavailable state:** without a fresh, known context (unpaired, link down, the plugin silent
  for 1.5 s, the generation unknown after a bind) every light that was lit from the last fresh context switches to the
  unavailable pattern: bars all ten LEDs blinking (red on RGB bars, blue on the parameter bars), red/green keys red
  slow-blinking, blue keys slow-blinking; a dark light stays dark (no write). No level or colour survives into
  staleness, and nothing is lit from a device report.
- **Bindings and colours change together:** a rebind (bank change, new generation) with no context for it yet is the
  same unknown state, and the first context of the new binding paints mapping and colour from one snapshot: no old
  colour identifies a newly mapped control (`a_new_mapping_without_its_context_carries_no_old_colour`).
- **Rate-limited, coalesced, never in the input path:** the loop handles reports, releases and link traffic first and
  renders the lights last; writes go out on change only and at most every `--lights-interval` (50 ms) per flush, a
  change in between waits for the next flush with the latest state (`coalesced` counts them). Each device's USB writer
  thread is unchanged (one control OUT at a time, input on its own reader thread). `--lights off` writes nothing but the
  KB-22 pickup span.
- **Textual state so colour is never the only cue:** `sim` prints one `light:` line per control at the end (`strip1
  slot1: ColorRGB_R "R" Color/RGB [ColorRGB] colour=Red (Definition; the strip's bar is blue-only) avail=available
  value=50% single (5/10)`, `M-Touch PB1 ex201: Sequence "Main" Go+/Master level=100% running colour=Yellow (Definition)
  -> Running`) and the `lights:` counters; `run --verbose` prints the lines whenever they change; `sim`'s `out ... bar`
  echo decodes the lanes (`G5`, `R10*`).
- **Per-device colour qualification:** what KB-16 saw on hardware is red, green, fast red and off on the pressure
  keys, red and green fills, blinking and clearing on the RGB bars, blue on the base-channel bars and keys. The mixes
  this scheme uses (yellow, cyan, magenta, white), blue on playback bars and the slow blink were **not** shown on
  hardware yet; they are documented approximations until KB-24's hardware pass (`docs/mtouch-protocol-reuse.md`).
- **Context cadence (plugin 0.9.0):** a change of what the service lights (values, levels, activity, colours, the
  known flag: `feedbackDigest`) resends the context after at most 250 ms (`feedbackMinMs`, coalesced) instead of only
  with the 1 s cadence; the binding generation is untouched by any of it (`counters.feedbackContexts`).
- **Rust service** (`cargo test` 90, eight new in `lights.rs` and `main.rs`): the colour derivation order and the
  palette reduction, the value indicators (no fabricated single level, mixed pattern, incomplete blink), the parameter
  strips, the playback states and level colours, staleness only where something was lit, the rebind rule, rate limiting
  / coalescing and the external bar write; a caller-level test that a bank change lights nothing from the previous
  binding's context until the new one arrives (review round 1); `banks::lights_controls` lists the lights of the
  present sections.
- **Binding readiness (review of PR #8):** the lights read `link::lighting_context` (not `context_view`), which is the
  fresh context only while no bind is in flight and, after a bind ack, a context of the acknowledged revision has
  arrived (`current_generation` is `Some`); a bank change's pending bind blocks it. The previous binding's context
  never lights the new mapping; `--verbose` logs why the lights are held unavailable (`lighting_block_reason`).

Harness: `lua tools/ma3/test/surface_plugin_test.lua` 237 checks (2 new: the executor's `col` from the stub sequence's
appearance, an activity change resending the context within 0.4 s with the generation unchanged); `sh
tools/ma3/test/e2e.sh` checks the lights summary, the textual lines (every bound executor empty on the stub console:
dark) and that the PFA1 key is written dark once per binding.

**Verified live on the console half:** the MCP probe (`node scripts/kb23-probe.mjs run`, 10/10 on 2026-10-10, record
`docs/probes/kb-23-colours-macos-2.5.1.md` there) established what this renderer's colours rest on: the show's
definitions colour R `1,0,0`, G, B, RY ("Amber") `1,0.5,0` and W; Dimmer/Pan/Gobo1 are black (category scheme); UV's
colour is out of gamut (`0.1744,-0.1443,1,1`, clamped by the module; the name override shows it as violet/magenta
anyway); appearances arrive as hex `FF7F00FF` ("Orange" -> yellow lanes). Both plugins (bridge with feedback 0.6.0,
`mtpnxk_surface` 0.9.0) are installed and running on `mcp-test-disposable`.

**Verified live in this repository:** [docs/probes/kb-23-surface-macos-2.5.1.md](docs/probes/kb-23-surface-macos-2.5.1.md)
(the plugin 0.9.0 on onPC with `control=console`, driven by the release service's `sim`): at rest the assigned keys lit
blue, the fader rows' PFDs red, the running executor's PFD green with its bar at ten LEDs in the yellow lanes (Orange
appearance); on the RGB page the strips' blue bars showed 5/10 for `ColorRGB_R` at 50 and 10/10 for the group-activated
G and B, each line naming the definition colour (Red/Green/Blue/Yellow); a two-fixture selection with different R values
showed the two end LEDs blinking and the touched strip's key fast-blinking; a PFA tap went blue fast while held and blue
slow once the console ran its Go+; stopping the plugin switched every lit control to the unavailable pattern (bars all
ten blinking red / blue, PFDs red slow, blue keys slow) and left the dark ones dark. Every setup change was undone.

**Review of PR #8 (round 1):** (1) the lights were gated at the caller on `link::lighting_context` so a rebind cannot
show the previous binding's feedback as current, with a caller-level regression and a verbose diagnostic; re-run live
(the steady scene lights `lit=21`, a bank change blanks the controls only until the matching context arrives). The
numeric-only hex finding (2) is the console half (gma3-mcp PR #27).

**Review of PR #8 (round 2):** a REFUSED bind (the plugin keeps its previous binding while the surface already maps the
new bank) left the old revision current, so `lighting_context` returned the old context and lit the new bank's controls
from the old bank's feedback (or showed them empty). `lighting_context` now also requires the context's reported
binding (`plugin_binding`) to equal the binding the surface asked for (`desired_bind`): a refused or not-yet-confirmed
bind keeps the lights unavailable until the plugin reports the requested binding (or the surface rolls back to a bound
one). A link-level test and the caller-level test (the bind refused) cover it; `cargo test` 92. A live bind refusal
cannot be induced with the normal 30-executor window (it needs the plugin's watch budget exceeded), so the path is
test-covered, not live-reproduced.

- **Rust service** (`cargo test` 92): two new tests for the refused-bind readiness; the lighting gate and its verbose
  diagnostic (`lighting_block_reason`) name the binding mismatch.

**Not verified live:** the refused-bind readiness path (test-covered; a live refusal needs the watch budget exceeded);
the physical M-Touch / M-Play rendering of the mixes, the slow blink and blue on playback bars (KB-24's hardware pass);
Windows/Linux hosts.

## KB-24 — Device profiles and end-to-end qualification (surface half, documentation and harness: 2026-10-10)

The console half is the MCP repository's note (ENCODERS.md "KB-24 results"): control 0.6.0 and 0.7.0, feedback 0.7.0
(the module defects the qualification found and the improvements that followed, 2026-10-11), the set hardkeys
0.10.0 / feedback 0.7.0 / control 0.7.0 at `bdcfe6e` (after the PR #28 review round 3).
This half has two parts; the first is done, the second needs the operator at the surfaces.

**Documentation (done):**

- [docs/device-profiles.md](docs/device-profiles.md): every physical control of the NX-K, the M-Touch and the
  M-Play with its default role, light and the flag that moves it; the bank arithmetic (row base + width · bank,
  banks 1–9 / 1–7 / 1–3); hot-plug, two surfaces and restart behaviour; what is unmapped (M-Touch command keys,
  MF keys, PFDs, M-Play SELECT/RELEASE/BEAT/SNAP, NX-K Snap Shot/Back/Swap Prog) and what is not claimed. The
  optional M-Play parameter strips of the acceptance are **not implemented** and said so.
- [docs/qualification-matrix.md](docs/qualification-matrix.md): hosts × console, devices × firmware/protocol
  source, the KB-24 scenarios, each row split into automated evidence and live qualification, and the proposed
  latency/loss thresholds with the results so far.

**Measurement harness (done):** `mtpnxk measure` (`service/src/measure.rs`): a repeating five-second workload on
the simulated surfaces at once (key taps, a Store/Clear feedback probe, two rotaries at 50 detents/s each, a
dragged parameter strip, a moved playback fader, an executor key) with a report of ack round trips by kind, motion
queueing, probe-to-state feedback latency, press-to-effect (plugin `bench`), loss/ordering counters, the plugin
loop's load, and a PASS/FAIL/n.a. verdict per threshold (`--json` for the record; exit 2 on a failure). To carry
it: the link records `rtt_key_ms` / `rtt_ctl_ms` / `motion_queue_ms` / `feedback_ms` (`probe_change`), and the
plugin (0.11.0) answers every heartbeat with `load { tmax, tavg, pmax, full, n }`, its loop's longest and mean
tick, the most datagrams one tick took and the ticks that hit the 32-per-tick budget since the previous reply
([docs/surface-protocol.md](docs/surface-protocol.md); protocol 2, optional field; 0.10.0 is the keyless localhost-only transport).

**Continuous control versus key releases (done):** `link.rs` proves that key events and boundaries are sent at
once while motion saturates the 16-per-tick budget across 40 controls (only the touched control's motion is
flushed before its boundary; the rest drains in order, nothing aged); the Lua suite proves that a release behind
31 ctl packets applies on the same plugin tick, that the per-session 400/s limit drops a ctl burst's excess and
that the key path is admitted afterwards. `cargo test` 96, `lua tools/ma3/test/surface_plugin_test.lua` 243,
`sh tools/ma3/test/e2e.sh` passed.

**Measured (2026-10-10) and fixed (2026-10-11):** [docs/probes/kb-24-measure-macos.md](docs/probes/kb-24-measure-macos.md).
Runs A–E against onPC 2.5.1 found three defects: the control module counted refused events as loss, its queue held
single detents and overflowed (`queue-full`) at 40 detents/s plus a strip, and at 100 detents/s the plugin saturated
and key taps were lost (103 lost, 101 superseded). Fixed: control 0.6.0 (refused numbers remembered per device and
not counted as loss; deltas merge into the most recent queued intent of their control across other controls'
motion; upstream `e4aa80a`, re-vendored), the service (one motion packet per control per 25 ms under a per-session
ceiling of 100/s, deferring and merging instead of dropping; retransmits after 120 ms; a release no longer drops a
press still inside its first ack window), the plugin 0.11.0 (8 intents per tick, state readers at most every 40 ms,
24 feedback reads per tick, the heartbeat load report with the tick's phases). Re-measured (run 3): admitted motion
passes every check (C3); 40 and 100 detents/s plus a strip plus keys lose nothing (E2, D2: 0 lost / superseded /
refused / retransmitted, key acks 67–69 ms p95); keys alone meet the agreed thresholds (B5). Thresholds agreed:
key and boundary ack p95 80 ms, worst 300 ms, plugin tick max 200 ms; every zero-loss line unchanged. Open: the
feedback read-back of a command-line keyword (median 60 ms, p95 509 ms, 11 of 39 probes missed), plugin tick spikes
of 266–275 ms under motion, and the load report's tick count exceeding the wall clock under traffic (the plugin's
own figures are indicative until understood). The service side stayed clean in every run (nothing stale or aged).

**Round 4 (2026-10-11, later):** control 0.7.0 merges a delta into a queued intent of its control before resolving the
target (within 100 ms of the intent), feedback 0.7.0 reads the state items before the executor targets (`priorityStateReads`,
half the budget, own round robin), the service enforces its ceiling per 100 ms, and the load report carries scheduling
diagnostics. Re-measured: feedback median 55 ms / p95 165 ms (was 60 / 509, 3 probes missed instead of 11); at 100
detents/s plus a strip plus keys the plugin never hits its datagram budget and the deferral is bounded at 44 ms (was
146); the admitted-motion and mixed runs pass (motion queue p95 agreed at 15 ms for the 25 ms interval). The scheduling
anomaly did not reproduce (one coroutine, no overlapping ticks, coherent sums): it coincided with a stale load table
surviving a plugin restart, now replaced. Open: press-to-effect p95 122 ms against 100, three missed probes, the context
collection every tick.

**Round 5 (2026-10-11, later):** the plugin reads and sends the console state on the tick after any key event;
press-to-effect p95 fell from 122 to 66 ms (B7), feedback 49 ms median / 172 ms p95. The "probes missed" line was
agreed at a tenth of the probes (3–5 of 39 are missed in every run since round 4; B7 is at 13 %). The context
collection stays per tick: gating it shifts the binding revision the KB-21/KB-22 tests expect, with the finding
recorded; the hardware pass is deferred until the merge.

**Review round 1 (PR #9 / gma3-mcp #28, 2026-10-11):** six findings fixed: the early merge runs behind every
admission guard (P1); both merge paths and the absolute supersede stop at another control's motion on the same
target; the refused-sequence set is bounded; the load window rotates at the end of the reporting tick so no tick is
lost; the measurement keeps one session through warm-up, workload and drain; the ordering verdict uses cumulative
per-code counters minus the warm-up snapshot. Re-measured: B8, C6 and E5 pass every check (feedback p95 141 ms, 2 of
39 probes missed); the 100 detents/s saturation run fails only the by-design deferral line and the plugin tick max
(247 ms, now honestly counted). `control_admission_test` 296, Lua 244, cargo 96, e2e.

**Pending (hardware pass, operator at the surfaces):** `measure` against onPC with the plugin 0.11.0 installed
(procedure in the record), the physical NX-K rotaries, M-Touch and
M-Play through `run` for every KB-16 … KB-23 scenario, two connected surfaces, USB removal, plugin restart, context
changes and stale feedback on the physical lights, the agreed thresholds, and the records
`docs/probes/kb-24-hardware-*.md`. Versions: service crate 0.1.0, plugin 0.11.0, protocol 2, module set
unchanged (VENDOR.md hashes equal upstream `plugin/modules.lock.json` at `9535ded`).

## KB-28 — Every qualified NX-K key as a Quickey (surface half, 2026-10-20)

**Request:** the shortcut-table route does not reach pop-ups and dialogs (an encoder's calculator, the Edit Command
pop-up), so the digits and `+ - . /` typed nothing there. Move every key that can be a Quickey to the Quickey path.

**Module half:** bdstark/gma3-mcp#74 (hardkeys 0.11.0, pinned at `30fe7d4`): a live probe qualified 24 more codes for
the executor-held Quickey path, with the record in `docs/probes/kb-28-quickey-codes-macos-2.5.1.md` there.

**Surface (plugin 0.12.0):**

- `input=mixed` routes a key through its Quickey whenever the loaded module's `BANK_QUALIFIED` gives its code hold
  evidence, instead of a fixed six-key list: the digits, `+ - . /`, `@`, `Thru`, `Full`, `Enter`, `Record`, `Clear`,
  `Update`, `Edit`, `Copy`, `Move`, `Delete`, `Cue`, `Group`, `Macro`, `Preview`, `HighLight`. `Undo` (tap-only
  evidence), `Next`, `Last` and `Menu` (not probed) stay on the shortcut table. A module without the table export
  falls back to the KB-10 list.
- At most one key goes down per plugin tick. Quickey presses issued in one console frame reorder in a focused text
  field (digits land after keywords, KB-10/KB-28); presses one frame apart keep their order. Every datagram is still
  read and admitted each tick (rate limit, heartbeats, other surfaces); key events behind a key-down wait in a bounded
  queue (64, then refused `overloaded`) in arrival order and are acknowledged when they run, their retransmissions
  ignored meanwhile; releases do not count against the tick (PR #12 review).
- The start log names the codes a kept bank lacks and the remedy (`bank teardown`, then `bank=...`) when the module
  reports them (`reprovision`), and the codes whose qualification the adoption refreshed.
- Tests: `surface_plugin_test.lua` 249 (the mixed and quickey sections follow the 0.11.0 qualification; two key-downs
  in one tick are dispatched one per tick in order, releases are not held back), `e2e.sh` passed.

**Live (onPC 2.5.1.0, `test-fixture-disposable`, `bank=900/1.178-189 input=mixed control=console`, the NX-K's
service running and a second simulated surface, `mtpnxk sim --id sim-kb28`):** the start reported 34 keys supported,
10 unsupported. With the Edit Command pop-up open, `1 Thru 5 2 + 3` sent with every key-down at the same instant
showed `1 Thru 52 +3`, in order; the service retransmitted the presses whose acknowledgement came after their release
and dropped those retransmissions (the originals were processed). Fixture 101 selected with `1 0 1 Enter`, the Dimmer
calculator opened, `6 2 . 5` (simultaneous downs) showed `62.5` and `Enter` stored it (the encoder bar read 63).
**Hardware (2026-10-20):** the operator typed a value into an encoder's calculator dialog on the physical NX-K
(`mtpnxk run`, same plugin start) and it was entered as expected.
