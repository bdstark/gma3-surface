# mtpnxk service

Cross-platform Rust process that owns the NX-K keypad over USB and talks to the
`mtpnxk_surface` plugin inside grandMA3 onPC over a localhost-only UDP link
([docs/surface-protocol.md](../docs/surface-protocol.md)).

```bash
cargo build --release
./target/release/mtpnxk run # drive the USB keypad
./target/release/mtpnxk sim --script "Record:down,5:tap@50,Record:up@300,Enter:tap"
./target/release/mtpnxk bench --taps 200 --rate 20
./target/release/mtpnxk measure --seconds 30 --json report.json   # KB-24 workload report
./target/release/mtpnxk list                        # USB devices
./target/release/mtpnxk mtouch-listen --seconds 240 # KB-16: decode an M-Touch / M-Play (--pid f808|f80c)
./target/release/mtpnxk mtouch-led-test             # KB-16: walk every LED, bar and display by eye
```

`--plugin 127.0.0.1:<port>` (default `127.0.0.1:9810`) selects the local plugin;
`localhost:<port>` is also accepted. Other addresses are refused. The service
binds its own UDP socket to `127.0.0.1`; run it on the onPC machine.
`--verbose` prints connection events, refusals and losses.

Start the console plugin without credentials:

```text
Plugin "mtpnxk_surface"
```

Protocol 2 (`MTX2`) requires plugin 0.10.0 and the matching service. Update both
together; remove old key options and network overrides. There is no key generation
or application-level authentication. Local processes are trusted, as with the MCP bridge.

## What it does

- `nxk`: decodes the keypad's interrupt packets (ported from the archived
  firmware, verified on hardware) and writes LEDs with the vendor control
  request. USB access is [nusb](https://crates.io/crates/nusb): no libusb.
  Windows needs the WinUSB driver bound to the keypad (Zadig); macOS and
  Linux need nothing (Linux may need a udev rule for non-root access).
- `link`: connection setup (hello/welcome with nonce), versioned JSON
  datagrams, strictly increasing sequence numbers, event ids retransmitted until
  acknowledged (4 × 60 ms, then counted lost; a press stops being retransmitted
  once its release is sent), heartbeats with the physically held keys,
  a 1.5 s watchdog, re-pairing after 4 s of silence or a `no-session` error,
  and the console-state cache keyed on the plugin generation and feedback
  epoch.
- `leds`: the NX-K map from `docs/ma3-feedback.md`: keyword keys follow
  `pending`, HighLight blinks while on, Preview follows the mode, Clear/Undo/
  Next/Last/Menu/Snap Shot echo the hold, Bank and the encoders light while
  Bank is held, Link blinks whenever the state is not fresh. Unknown is off.
- `mtouch`: the Martin M-Touch (`11be:f808`) and M-Play (`11be:f80c`):
  control tables for both models, the decoder for the four report types
  (single key, single fader, key bank, analog bank; bank unit count read from
  the packet; concatenated reports split), and pure output encoders for LED
  keys (`0x80`), fader bars (`0x61`) and the page display (`0x54`), ported
  from MTouchPlay `e48eb2c` and regression-tested against its capture logs.
  `mtouch-listen` and `mtouch-led-test` are the operator's hardware
  qualification tools ([docs/mtouch-protocol-reuse.md](../docs/mtouch-protocol-reuse.md));
  neither device has been connected to this service and neither is wired into
  the link (`run`, `sim` and `bench` are NX-K only).
- `sim`: a scripted keypad for machines without the hardware; `bench` is a
  scripted tap run that prints ack round-trip percentiles and, when the plugin
  was started with `bench`, press-to-effect percentiles.
- `measure` (KB-24): a simultaneous workload on the simulated NX-K and M-Touch (key taps,
  a Store/Clear feedback probe, two rotaries, a parameter strip, a playback fader and
  key) with a report of latencies, loss, ordering and the plugin loop's load against the
  thresholds in `docs/qualification-matrix.md` (`--json` for the record; exit 2 on a failure).

`cargo test` runs the unit tests, including a fake plugin that exercises the
link's retransmission, loss, freshness and re-pairing paths and 17 M-Touch /
M-Play regressions against the documented captures. Operator setup and
recovery: [docs/operator-guide.md](../docs/operator-guide.md); qualified
deployments: [docs/deployments.md](../docs/deployments.md).
