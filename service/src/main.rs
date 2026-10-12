//! mtpnxk: bridges an Obsidian/Elation NX-K keypad to the mtpnxk_surface plugin inside grandMA3
//! onPC over a localhost-only UDP link. Protocol and design: docs/surface-protocol.md.

mod wire;
mod banks;
mod leds;
mod lights;
mod link;
mod measure;
mod mtouch;
mod nxk;
mod protocol;
mod sim;
mod strips;
mod usb_surfaces;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use nxk::usb::Surface;
use nxk::Event;
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(name = "mtpnxk", version, about = "NX-K keypad to grandMA3 onPC surface plugin")]
struct Cli {
    /// Local plugin endpoint (127.0.0.1:port or localhost:port); remote addresses are refused.
    #[arg(long, default_value = "127.0.0.1:9810", global = true)]
    plugin: String,
    /// Surface id reported to the plugin.
    #[arg(long, global = true)]
    id: Option<String>,
    /// Print every link event (pairing, refusals, losses).
    #[arg(long, global = true)]
    verbose: bool,
    /// KB-19: the first encoder slot the four rotaries mean (1 = slots 1-4, the console's default page
    /// layout; 2 = slots 2-5 reaches a fifth pool slot). An explicit window, never a silent discard; a slot
    /// the bound page does not have is refused by the plugin with the reason.
    #[arg(long, default_value_t = 1, global = true, value_parser = clap::value_parser!(u8).range(1..=5))]
    rotary_slots: u8,
    /// KB-20: what the M-Touch's four base-channel strips do: `relative` (default: touch anchors, the drag adjusts
    /// the slot like a rotary, lift and retouch re-anchors), `absolute` (positions after pickup or an explicit
    /// takeover with the strip's key held at touch-down) or `off`.
    #[arg(long, default_value = "relative", global = true)]
    strips: strips::Mode,
    /// KB-20: the first encoder slot the four strips mean (as --rotary-slots for the rotaries).
    #[arg(long, default_value_t = 1, global = true, value_parser = clap::value_parser!(u8).range(1..=5))]
    strip_slots: u8,
    /// KB-20: detents across a strip's full travel (120 = the five encoder turns of one attribute range).
    #[arg(long, default_value_t = 120, global = true, value_parser = clap::value_parser!(i32).range(1..=4096))]
    strip_travel: i32,
    /// KB-20: `auto` opens an attached M-Touch next to the NX-K in `run` (retrying while it is absent); `off` ignores it.
    #[arg(long, default_value = "auto", global = true)]
    mtouch: String,
    /// KB-21: `auto` opens an attached M-Play too (its playback buttons and bank keys); `off` ignores it.
    #[arg(long, default_value = "auto", global = true)]
    mplay: String,
    /// KB-21: which executor page the playback banks mean: `follow` (default: the console's current page; a page
    /// change on the console moves the targets) or a page number (independent: that page whatever the console
    /// shows; it is never created or selected by the service).
    #[arg(long, default_value = "follow", global = true)]
    pb_page: banks::PageMode,
    /// KB-21: the executor rows a playback strip maps to, `<keyA>,<fader>,<keyB>` row bases (default 101,201,301:
    /// the button above, the fader's own executor, the button below). A bank of n strips moves by n within a row.
    #[arg(long, default_value = "101,201,301", global = true)]
    pb_rows: banks::Rows,
    /// KB-21: the executor row of the M-Play's right block (24 buttons, banks of 24).
    #[arg(long, default_value_t = 401, global = true, value_parser = clap::value_parser!(u32).range(1..=901))]
    pb_ext_row: u32,
    /// KB-21: `on` binds the playback banks (the plugin reads their executors); `off` leaves playback controls unmapped.
    #[arg(long, default_value = "on", global = true)]
    banks: String,
    /// KB-22: a playback fader position within this distance (0..1 of the travel) of the executor's last known level
    /// picks it up; until then the strip moves nothing and its bar shows the span to travel.
    #[arg(long, default_value_t = 0.02, global = true)]
    pb_pickup: f64,
    /// KB-22: the executor the `Go` key of the M-Touch / M-Play presses (its key element, whatever button
    /// function is configured on it; on the bound page, never banked). Unmapped without it.
    #[arg(long, global = true, value_parser = clap::value_parser!(u32).range(1..=999))]
    pb_go: Option<u32>,
    /// KB-22: the executor the `Pause (||/Back)` key presses (as --pb-go). Unmapped without it.
    #[arg(long, global = true, value_parser = clap::value_parser!(u32).range(1..=999))]
    pb_pause: Option<u32>,
    /// KB-23: `on` lights the M-Touch / M-Play keys and bars from the console context (parameter values on the
    /// blue strip bars, playback assignment / running / pressed / level in the appearance colour, an unavailable
    /// pattern while the context is stale); `off` writes no light but the KB-22 pickup span.
    #[arg(long, default_value = "on", global = true)]
    lights: String,
    /// KB-23: the minimum interval between two flushes of light writes, in milliseconds (coalescing bound).
    #[arg(long, default_value_t = 50, global = true, value_parser = clap::value_parser!(u64).range(1..=1000))]
    lights_interval: u64,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Drive the USB keypad.
    Run,
    /// Play a key script without hardware (see src/sim.rs for the syntax); prints LED writes.
    Sim {
        #[arg(long, default_value = "Record:down,5:tap@50,Record:up@300,Enter:tap,Clear:tap@200")]
        script: String,
        #[arg(long)]
        repeat: bool,
        /// Seconds to run before exiting (0 = until the script ends, or forever with --repeat).
        #[arg(long, default_value_t = 0.0)]
        seconds: f64,
        /// Exit without saying bye (simulates a pulled cable; the plugin's lease must clean up).
        #[arg(long)]
        abandon: bool,
    },
    /// Latency measurement: taps of '5' (and a Clear every 10) at a rate; reports percentiles.
    /// Start the plugin with `bench` to get press-to-effect timings as well.
    Bench {
        #[arg(long, default_value_t = 200)]
        taps: u32,
        /// Taps per second.
        #[arg(long, default_value_t = 20.0)]
        rate: f64,
    },
    /// KB-24 measurement harness: a realistic simultaneous workload on the simulated surfaces (key taps and a
    /// feedback probe on the NX-K, two rotaries turning, an M-Touch parameter strip dragged, a playback fader
    /// moved and a playback button tapped) for a number of seconds. Reports input latency (ack round trips by
    /// kind, motion queueing), feedback latency (probe key to console state change), press-to-effect when the
    /// plugin runs with `bench`, missed events, ordering, the plugin loop's load, and judges them against the
    /// KB-24 thresholds (docs/qualification-matrix.md). `--json` writes the report for the records.
    Measure {
        /// Seconds of workload (after pairing and the first context).
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
        /// Key taps per second ('5'; every tenth tap is a Clear).
        #[arg(long, default_value_t = 4.0)]
        key_rate: f64,
        /// Detents per second on each of rotaries 2 and 3 (one detent per report; slot 1 is the strip's).
        #[arg(long, default_value_t = 50.0)]
        motion_rate: f64,
        /// Feedback probes per second: a tap of --probe-key, each followed by a Clear.
        #[arg(long, default_value_t = 2.0)]
        probe_rate: f64,
        /// The NX-K key whose press must change --probe-item in the console state (Record puts "Store" on the
        /// command line; HighLight with item highlight is the alternative).
        #[arg(long, default_value = "Record")]
        probe_key: String,
        #[arg(long, default_value = "pending")]
        probe_item: String,
        /// Leave the M-Touch parameter strip out of the workload.
        #[arg(long)]
        no_strip: bool,
        /// Leave the playback fader and button out of the workload.
        #[arg(long)]
        no_playback: bool,
        /// Write the report (samples summarised, counters, verdicts) as JSON here.
        #[arg(long)]
        json: Option<std::path::PathBuf>,
    },
    /// List USB devices.
    List,
    /// KB-16: print every report of an M-Touch or M-Play (decoded, with the raw packet) and a
    /// summary on exit. Not wired into the link; the operator's hardware qualification tool.
    MtouchListen {
        /// Seconds to listen (0 = until the device goes away or the process is killed).
        #[arg(long, default_value_t = 30.0)]
        seconds: f64,
        /// Product id to open: f808 (M-Touch) or f80c (M-Play); default: the first of either.
        #[arg(long)]
        pid: Option<String>,
    },
    /// KB-16: walk every LED key, fader bar and page display of an M-Touch or M-Play with a
    /// deterministic sequence, printing each write so an operator can confirm it by eye.
    MtouchLedTest {
        #[arg(long)]
        pid: Option<String>,
        /// Milliseconds each step is held.
        #[arg(long, default_value_t = 300)]
        hold_ms: u64,
    },
}

// Resolve no arbitrary hostname: only the fixed IPv4 loopback endpoint is supported.
fn plugin_address(value: &str) -> Result<SocketAddrV4> {
    let (host, port) = value.rsplit_once(':').ok_or_else(|| anyhow!("--plugin must be localhost:<port> or 127.0.0.1:<port>"))?;
    if host != "127.0.0.1" && host != "localhost" {
        return Err(anyhow!("the surface plugin is localhost-only; use 127.0.0.1:<port>"));
    }
    let port: u16 = port.parse().context("plugin port must be 1..65535")?;
    if port == 0 { return Err(anyhow!("plugin port must be 1..65535")); }
    Ok(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn surface_id(cli: &Cli) -> String {
    cli.id.clone().unwrap_or_else(|| {
        let host = std::env::var("HOSTNAME").ok().or_else(|| std::env::var("COMPUTERNAME").ok()).unwrap_or_else(|| "host".into());
        format!("nxk-{host}")
    })
}

struct Loop {
    link: link::Link,
    leds: leds::Renderer,
    sock: UdpSocket,
    started: Instant,
    verbose: bool,
    log_seen: usize,
    bank_held: bool,
    bye_on_exit: bool,
    /// KB-19: the first encoder slot of the rotaries (`--rotary-slots`).
    rotary_base: u8,
    /// KB-20: the M-Touch parameter strips' gesture converter.
    strips: strips::Strips,
    strips_log_seen: usize,
    /// Rotary pushes currently down (released for the keypad when it is lost).
    pushed_wheels: BTreeSet<u8>,
    /// KB-21: the playback banks (control -> executor mapping, frozen holds, displays).
    banks: banks::Banks,
    banks_log_seen: usize,
    /// What each page display last showed (written only on change).
    displays: BTreeMap<u16, Option<u16>>,
    /// KB-22: fader bar writes produced while handling reports (flushed after the LEDs).
    bar_writes: Vec<(mtouch::Model, mtouch::Output)>,
    /// KB-23: the M-Touch / M-Play lights renderer (keys and bars from the console context).
    lights: lights::Lights,
    lights_lines_seen: Vec<String>,
    lights_block_seen: Option<String>,
    present: BTreeMap<mtouch::Model, bool>,
    /// Hardware pass: playback keys waiting for the console to report their effect: (executor, page) -> (the
    /// activity expected, when the key moved, the control's name). See `time_playback_keys`.
    key_timing: BTreeMap<(u32, Option<u32>), (bool, f64, &'static str)>,
    /// Press/release-to-activity samples in milliseconds (logged and kept for the summary).
    pub key_effect_ms: Vec<f64>,
    /// KB-24: the measurement harness's feedback probe: a press of this NX-K key opens a probe on this console
    /// state item (the time until the item changes is a feedback-latency sample).
    probe: Option<(&'static str, String)>,
    /// The fine flag handed to the strips converter by the last strip report (what a stale Bank would corrupt).
    #[cfg(test)]
    last_strip_fine: Option<bool>,
}

impl Loop {
    fn new(cli: &Cli) -> Result<Loop> {
        let sock = UdpSocket::bind("127.0.0.1:0").context("binding a UDP socket")?;
        sock.connect(plugin_address(&cli.plugin)?).with_context(|| format!("resolving the plugin address {}", cli.plugin))?;
        sock.set_nonblocking(true)?;
        let cfg = link::Config { id: surface_id(cli), ..link::Config::default() };
        let started = Instant::now();
        if cli.rotary_slots != 1 {
            eprintln!("rotaries: explicit slot window {}-{} (--rotary-slots {})", cli.rotary_slots, cli.rotary_slots + 3, cli.rotary_slots);
        }
        let strip_cfg = strips::Config { mode: cli.strips, base_slot: cli.strip_slots, detents_per_travel: cli.strip_travel, ..strips::Config::default() };
        if cli.strips != strips::Mode::Off {
            eprintln!("strips: {:?} on slots {}-{}, {} detents per travel (--strips/--strip-slots/--strip-travel)", cli.strips, cli.strip_slots, cli.strip_slots + 3, cli.strip_travel);
        }
        let banks_on = match cli.banks.to_ascii_lowercase().as_str() {
            "on" | "yes" => true,
            "off" | "no" => false,
            other => return Err(anyhow!("--banks {other}: expected on or off")),
        };
        let mplay_on = match cli.mplay.to_ascii_lowercase().as_str() {
            "auto" => true,
            "off" | "none" => false,
            other => return Err(anyhow!("--mplay {other}: expected auto or off")),
        };
        if !(0.0..=0.5).contains(&cli.pb_pickup) {
            return Err(anyhow!("--pb-pickup {}: expected a tolerance in 0..0.5 of the travel", cli.pb_pickup));
        }
        let bank_cfg = banks::Config { enabled: banks_on, mode: cli.pb_page, rows: cli.pb_rows, ext_row: cli.pb_ext_row, mplay: mplay_on,
                                       pickup_tolerance: cli.pb_pickup, go_executor: cli.pb_go, pause_executor: cli.pb_pause, ..banks::Config::default() };
        let banks = banks::Banks::new(bank_cfg);
        if banks_on {
            eprintln!("{} (--pb-page/--pb-rows/--pb-ext-row/--banks; faders pick up within {} of the executor's level, --pb-pickup; Go/Pause keys {})", banks.summary(None), cli.pb_pickup,
                      match (cli.pb_go, cli.pb_pause) { (None, None) => "unmapped (--pb-go/--pb-pause)".to_string(), (g, p) => format!("go={} pause={}", g.map(|n| n.to_string()).unwrap_or_else(|| "-".into()), p.map(|n| n.to_string()).unwrap_or_else(|| "-".into())) });
        }
        let lights_on = match cli.lights.to_ascii_lowercase().as_str() {
            "on" | "yes" => true,
            "off" | "no" => false,
            other => return Err(anyhow!("--lights {other}: expected on or off")),
        };
        if lights_on {
            eprintln!("lights: on (--lights/--lights-interval {} ms): parameter values on the blue strip bars, playback assignment/running/pressed/level on the keys and RGB bars, an unavailable pattern while the context is stale", cli.lights_interval);
        }
        let link = link::Link::new(cfg, 0.0);
        Ok(Loop { link, leds: leds::Renderer::new(), sock, started, verbose: cli.verbose, log_seen: 0, bank_held: false, bye_on_exit: true, rotary_base: cli.rotary_slots,
                  strips: strips::Strips::new(strip_cfg), strips_log_seen: 0, pushed_wheels: BTreeSet::new(),
                  banks, banks_log_seen: 0, displays: BTreeMap::new(), bar_writes: Vec::new(),
                  lights: lights::Lights::new(lights::Config { enabled: lights_on, min_interval: cli.lights_interval as f64 / 1000.0 }), lights_lines_seen: Vec::new(), lights_block_seen: None, present: BTreeMap::new(), key_timing: BTreeMap::new(), key_effect_ms: Vec::new(), probe: None,
                  #[cfg(test)]
                  last_strip_fine: None })
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// One iteration: surface events, datagrams, link timers, sends, LEDs.
    fn step(&mut self, surface: &mut dyn Surface) -> Result<()> {
        let now = self.now();
        if surface.reconnected() {
            self.leds.invalidate();
        }
        let events = surface.poll()?;
        for ev in events {
            match ev {
                Event::KeyDown { name, .. } => {
                    if name == "Bank" {
                        self.bank_held = true;
                    }
                    self.leds.key_event(name, true, now);
                    if let Some((k, item)) = &self.probe {
                        if *k == name {
                            let item = item.clone();
                            self.link.probe_change(&item, now);
                        }
                    }
                    self.link.key_event(name, true, now);
                }
                Event::KeyUp { name, .. } => {
                    if name == "Bank" {
                        self.bank_held = false;
                    }
                    self.link.key_event(name, false, now);
                }
                // KB-18: the four rotaries are four encoder slots of the bound display (slots
                // `--rotary-slots`..+3, 1-4 by default); Bank held is the explicit fine modifier (KB-19: a
                // tenth of a click on the console); a push is a button boundary (refused by the console
                // backend: not qualified). Everything travels as `ctl` events through the link's admission.
                // No acceleration is applied anywhere in the service: the device's delta is the detent count.
                Event::Rotate { wheel, delta, .. } => self.link.control_event("nxk", ROTARIES[(wheel.clamp(1, 4) - 1) as usize], link_target(wheel, self.rotary_base), link::CtlKind::Rel { dx: delta, fine: self.bank_held }, now),
                Event::PressDown { wheel, .. } => {
                    self.pushed_wheels.insert(wheel.clamp(1, 4));
                    self.link.control_event("nxk", ROTARIES[(wheel.clamp(1, 4) - 1) as usize], link_target(wheel, self.rotary_base), link::CtlKind::Btn { down: true }, now)
                }
                Event::PressUp { wheel, .. } => {
                    self.pushed_wheels.remove(&wheel.clamp(1, 4));
                    self.link.control_event("nxk", ROTARIES[(wheel.clamp(1, 4) - 1) as usize], link_target(wheel, self.rotary_base), link::CtlKind::Btn { down: false }, now)
                }
                Event::Unknown { bytes } => {
                    if self.verbose {
                        eprintln!("nxk: unknown packet {bytes:02x?}");
                    }
                }
            }
        }
        // KB-20 (review): the keypad went away after whatever it sent above; what it still held is released.
        if surface.disconnected() {
            self.release_keypad(now);
        }
        // KB-20: the M-Touch's parameter strips. The converter emits the same control events the rotaries do;
        // the binding generation and the slot's last known value come from the link's context.
        let strip_events = match surface.poll_strips() {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("mtouch: {e:#}; its strip touches are ended and its playback buttons released; retrying every 2 s");
                let outs = self.strips.disconnect();
                self.emit_strip_outs(outs, now);
                // Review (PR #6): the M-Touch's held playback buttons are released against their frozen targets too
                // (the M-Play's holds are untouched).
                for o in self.banks.disconnect("mtouch") {
                    self.emit_bank_out(Some(o), now);
                }
                Vec::new()
            }
        };
        for ev in strip_events {
            match ev {
                mtouch::Event::Fader { id, value, touch, .. } => {
                    if let Some(n) = strips::strip_of_fader(id) {
                        let cg = self.link.current_generation(now);
                        let slot = self.strips.cfg.base_slot.clamp(1, 5) + n - 1;
                        let hint = self.link.slot_hint(slot, now);
                        #[cfg(test)]
                        {
                            self.last_strip_fine = Some(self.bank_held);
                        }
                        let outs = self.strips.fader(n, value, touch, self.bank_held, cg, hint, now);
                        self.emit_strip_outs(outs, now);
                    } else if let Some((section, strip)) = banks::playback_fader_of(id) {
                        self.playback_fader(section, strip, value, touch, now);
                    } else {
                        self.strips.stats.ignored += 1;
                    }
                }
                mtouch::Event::KeyDown { id, .. } | mtouch::Event::KeyUp { id, .. } => {
                    let down = matches!(ev, mtouch::Event::KeyDown { .. });
                    if let Some(n) = strips::strip_of_key(id) {
                        self.strips.key(n, down);
                    } else {
                        // KB-21: playback buttons and the bank keys; KB-22: the dedicated Go/Pause keys through the profile.
                        let out = self.banks.key_on("mtouch", id, down);
                        self.emit_bank_out(out, now);
                    }
                }
                // KB-22: the M-Touch's pressure keys (PFD, MF) are counted unmapped once per press; never a strip input (KB-20).
                mtouch::Event::Pressure { id, value, touch, .. } => { let o = self.banks.pressure("mtouch", id, value, touch); self.emit_bank_out(o, now); }
            }
        }
        // KB-21: the M-Play's playback buttons, bank keys and (counted) faders.
        let mplay_events = match surface.poll_mplay() {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("mplay: {e:#}; its playback buttons and faders are released; retrying every 2 s");
                let outs = self.banks.disconnect("mplay");
                for o in outs {
                    self.emit_bank_out(Some(o), now);
                }
                Vec::new()
            }
        };
        for ev in mplay_events {
            match ev {
                // KB-22: the M-Play's twelve faders by id; its playback buttons are pressure keys (one down, one up).
                mtouch::Event::Fader { id, value, touch, .. } => {
                    if let Some((section, strip)) = banks::playback_fader_of(id) { self.playback_fader(section, strip, value, touch, now); } else { self.banks.stats.ignored_faders += 1; }
                }
                mtouch::Event::KeyDown { id, .. } => { let o = self.banks.key_on("mplay", id, true); self.emit_bank_out(o, now); }
                mtouch::Event::KeyUp { id, .. } => { let o = self.banks.key_on("mplay", id, false); self.emit_bank_out(o, now); }
                mtouch::Event::Pressure { id, value, touch, .. } => { let o = self.banks.pressure("mplay", id, value, touch); self.emit_bank_out(o, now); }
            }
        }
        // KB-21: only attached devices' bank windows are bound (an absent M-Play's 60 executors are not watched).
        for model in [mtouch::Model::MTouch, mtouch::Model::MPlay] {
            let present = surface.device_present(model);
            self.banks.set_present(model, present);
            // KB-23: a device that (re)appeared gets every light rewritten.
            if self.present.insert(model, present) != Some(present) && present {
                self.lights.invalidate(model);
            }
        }
        if self.banks.take_rebind() && self.banks.cfg.enabled {
            self.link.set_binding(self.banks.targets(), now);
        }
        // KB-21: the plugin replaced its binding (our bank change applied, or its own rebind): a touched parameter
        // strip is rebound here as it is there; absolute mode picks up again on the next touch.
        if self.link.take_binding_changed() {
            self.strips.rebind_all();
            let outs = self.banks.rebind_faders();
            self.emit_bank_outs(outs, now);
        }
        let cg = self.link.current_generation(now);
        let outs = self.strips.tick(cg, now);
        self.emit_strip_outs(outs, now);
        let outs = self.banks.tick(cg, now);
        self.emit_bank_outs(outs, now);
        let mut buf = [0u8; 16384];
        loop {
            match self.sock.recv(&mut buf) {
                Ok(n) => self.link.receive(&buf[..n], self.now()),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::ConnectionReset || e.kind() == ErrorKind::ConnectionRefused => break,
                Err(e) => return Err(e).context("receiving"),
            }
        }
        self.link.tick(self.now());
        for d in self.link.take_outgoing() {
            if let Err(e) = self.sock.send(&d) {
                if e.kind() != ErrorKind::ConnectionRefused && e.kind() != ErrorKind::ConnectionReset {
                    return Err(e).context("sending");
                }
            }
        }
        let now = self.now();
        let view = self.link.console(now);
        let held: BTreeSet<&'static str> = self.link.held().clone();
        for (id, v) in self.leds.render(&view, &held, now) {
            surface.led(id, v);
        }
        // KB-22: the pickup display of the playback faders' bars (banks emits a write on change only).
        for (model, out) in std::mem::take(&mut self.bar_writes) {
            self.lights.note_external(model, &out);
            surface.output(model, out);
        }
        // KB-23: the keys and bars from the console context, change-only and rate-limited; input and releases were
        // handled above, so a flush never delays them.
        {
            // Review of PR #8: the context must belong to the binding in effect (no bind in flight, the acknowledged
            // revision's context arrived); the previous binding's context never lights the new mapping.
            let ctx = self.link.lighting_context(now).map(|c| &c.body);
            let strips_present = surface.device_present(mtouch::Model::MTouch);
            let strips = if self.strips.cfg.mode != strips::Mode::Off {
                Some(lights::ParamStrips { present: strips_present, base_slot: self.strips.cfg.base_slot, touching: [self.strips.touching(1), self.strips.touching(2), self.strips.touching(3), self.strips.touching(4)], echo: [self.strips.echo_at(1, self.link.current_generation(now)), self.strips.echo_at(2, self.link.current_generation(now)), self.strips.echo_at(3, self.link.current_generation(now)), self.strips.echo_at(4, self.link.current_generation(now))] })
            } else {
                None
            };
            // Review of PR #10 (round 2): the echoes are offered for the generation of the context rendered now, which
            // includes this step's incoming packets; a gesture started against an older generation offers none.
            let render_cg = self.link.current_generation(now);
            let playback = self.banks.lights_controls(render_cg);
            // Hardware pass: a missing context while the link holds a fresh one is a rebind in flight, not staleness.
            self.lights.set_rebinding(ctx.is_none() && self.link.in_transition(now));
            self.lights.set_generation(self.link.current_generation(now));
            let writes = self.lights.render(&lights::Scene { ctx, strips, playback: &playback }, now);
            for (model, out) in writes {
                surface.output(model, out);
            }
            if self.verbose {
                let reason = self.link.lighting_block_reason(now);
                if reason != self.lights_block_seen {
                    match &reason {
                        Some(r) => eprintln!("lights: unavailable although a context exists: {r}"),
                        None => eprintln!("lights: the context lights again"),
                    }
                    self.lights_block_seen = reason;
                }
            }
            if self.verbose && self.lights.lines != self.lights_lines_seen {
                for l in &self.lights.lines {
                    eprintln!("lights: {l}");
                }
                self.lights_lines_seen = self.lights.lines.clone();
            }
        }
        if !self.key_timing.is_empty() {
            self.time_playback_keys(now);
        }
        // KB-21: the page displays show "page then bank" (blank while the page is unknown); written on change only.
        if self.banks.cfg.enabled {
            let page = self.link.console_page(now);
            for i in 0..self.banks.sections.len() {
                let s = &self.banks.sections[i];
                if !s.enabled {
                    continue;
                }
                let v = self.banks.display_value(i, page);
                if self.displays.get(&s.display) != Some(&v) {
                    self.displays.insert(s.display, v);
                    surface.output(s.model, mtouch::Output::PageDisplay { id: s.display, data: mtouch::page_bank_display(v) });
                }
            }
        }
        if self.verbose {
            while self.log_seen < self.link.log.len() {
                eprintln!("link: {}", self.link.log[self.log_seen]);
                self.log_seen += 1;
            }
            while self.strips_log_seen < self.strips.log.len() {
                eprintln!("strips: {}", self.strips.log[self.strips_log_seen]);
                self.strips_log_seen += 1;
            }
            while self.banks_log_seen < self.banks.log.len() {
                eprintln!("banks: {}", self.banks.log[self.banks_log_seen]);
                self.banks_log_seen += 1;
            }
        }
        Ok(())
    }

    /// KB-20 (review of PR #5): the keypad is gone, so it can never send its releases. Every key the link holds
    /// for it is released (a pending press is superseded by its release, as for a physical release), every rotary
    /// push is ended and the Bank modifier is cleared so the strips stop reading as fine. The M-Touch's own
    /// gestures are untouched: a finger on a strip is still there.
    fn release_keypad(&mut self, now: f64) {
        let held: Vec<&'static str> = self.link.held().iter().copied().collect();
        for name in &held {
            self.link.key_event(name, false, now);
        }
        let pushed: Vec<u8> = self.pushed_wheels.iter().copied().collect();
        for wheel in pushed {
            self.link.control_event("nxk", ROTARIES[(wheel - 1) as usize], link_target(wheel, self.rotary_base), link::CtlKind::Btn { down: false }, now);
        }
        self.pushed_wheels.clear();
        self.bank_held = false;
        if !held.is_empty() || self.verbose {
            eprintln!("nxk: lost with {} key(s) held ({}): released", held.len(), held.join(", "));
        }
    }

    /// KB-22: one playback fader report: the executor's last known level comes from the link's context.
    fn playback_fader(&mut self, section: usize, strip: u32, value: u8, touch: bool, now: f64) {
        let cg = self.link.current_generation(now);
        let (ex, pg) = self.banks.fader_executor(section, strip);
        let hint = self.link.exec_hint(ex, pg, now);
        let outs = self.banks.fader(section, strip, value, touch, cg, hint.as_ref(), now);
        self.emit_bank_outs(outs, now);
    }

    fn emit_bank_outs(&mut self, outs: Vec<banks::Out>, now: f64) {
        for o in outs {
            self.emit_bank_out(Some(o), now);
        }
    }

    fn emit_strip_outs(&mut self, outs: Vec<strips::Out>, now: f64) {
        for o in outs {
            self.link.control_event("mtouch", o.control, o.target, o.kind, now);
        }
    }

    /// KB-21: a playback button goes to the link as a `btn` event with its (frozen) executor target; a bank change
    /// is logged (the rebind goes out from `step`).
    fn emit_bank_out(&mut self, out: Option<banks::Out>, now: f64) {
        match out {
            Some(banks::Out::Button { dev, control, target, kind }) => {
                if let (protocol::CtlTarget::Executor { ex, pg, .. }, link::CtlKind::Btn { down }) = (target, kind) {
                    self.note_key_timing(ex, pg, down, control, now);
                }
                self.link.control_event(dev, control, target, kind, now)
            }
            Some(banks::Out::Fader { dev, control, target, kind }) => self.link.control_event(dev, control, target, kind, now),
            Some(banks::Out::Bar { model, id, data }) => self.bar_writes.push((model, mtouch::Output::FaderBar { id, data })),
            Some(banks::Out::Bank { .. }) | None => {}
        }
    }

    /// The fresh context's entry for a bound executor; None when there is no fresh context or it does not list it.
    fn exec_entry(&self, ex: u32, pg: Option<u32>, now: f64) -> Option<&serde_json::Value> {
        let ctx = self.link.context_view(now)?;
        let exs = ctx.body.get("ex")?.as_array()?;
        exs.iter().find(|x| x.get("n").and_then(|n| n.as_u64()) == Some(ex as u64) && x.get("pg").and_then(|p| p.as_u64()).map(|p| p as u32) == pg)
    }

    /// Review of PR #11: whether a fresh context lists the executor (Some(false): it left the binding); None without a
    /// fresh context, when membership is unknown.
    fn exec_bound(&self, ex: u32, pg: Option<u32>, now: f64) -> Option<bool> {
        self.link.context_view(now)?;
        Some(self.exec_entry(ex, pg, now).is_some())
    }

    /// The context's activity of a bound executor (`act` 1/0) and its key function; None when it is not listed or its
    /// activity is unreadable (`act` "?").
    fn exec_activity(&self, ex: u32, pg: Option<u32>, now: f64) -> Option<(bool, Option<String>)> {
        let x = self.exec_entry(ex, pg, now)?;
        let act = x.get("act").and_then(|v| v.as_u64())? == 1;
        Some((act, x.get("kp").and_then(|v| v.as_str()).map(|s| s.to_string())))
    }

    /// Hardware pass: start timing a playback key. A down expects the executor to start running (skipped when it
    /// already runs); an up expects it to stop only for the momentary functions (Flash, Temp).
    fn note_key_timing(&mut self, ex: u32, pg: Option<u32>, down: bool, control: &'static str, now: f64) {
        let Some((act, kp)) = self.exec_activity(ex, pg, now) else { return };
        let momentary = matches!(kp.as_deref(), Some("Flash") | Some("Temp"));
        if down && !act {
            self.key_timing.insert((ex, pg), (true, now, control));
        } else if !down && act && momentary {
            self.key_timing.insert((ex, pg), (false, now, control));
        } else {
            self.key_timing.remove(&(ex, pg));
        }
    }

    /// Hardware pass: log how long each timed key took to show as running/stopped in the console's context (what the
    /// lights follow), or that it did not within 3 s.
    fn time_playback_keys(&mut self, now: f64) {
        let keys: Vec<(u32, Option<u32>)> = self.key_timing.keys().copied().collect();
        for k in keys {
            let (want, at, control) = self.key_timing[&k];
            if self.exec_bound(k.0, k.1, now) == Some(false) {
                // M-Play pass (review of PR #11): the executor left the binding (its device was unplugged, its window
                // unbound) before its change was reported; that is not a slow console, so no timing is claimed. A bound
                // executor whose activity is momentarily unreadable keeps waiting (readable activity or the timeout).
                eprintln!("playback: {control} -> executor {}: no longer bound; not timed", k.0);
                self.key_timing.remove(&k);
                continue;
            }
            let seen = self.exec_activity(k.0, k.1, now).map(|(a, _)| a == want).unwrap_or(false);
            if seen {
                let ms = (now - at) * 1000.0;
                self.key_effect_ms.push(ms);
                eprintln!("playback: {control} -> executor {}: {} {:.0} ms after the key went {}", k.0, if want { "running" } else { "stopped" }, ms, if want { "down" } else { "up" });
                self.key_timing.remove(&k);
            } else if now - at > 3.0 {
                eprintln!("playback: {control} -> executor {}: not reported {} within 3 s", k.0, if want { "running" } else { "stopped" });
                self.key_timing.remove(&k);
            }
        }
    }

    fn run(&mut self, surface: &mut dyn Surface, until: impl Fn(&Loop, &dyn Surface) -> bool) -> Result<()> {
        eprintln!("mtpnxk {}: {} -> plugin at {}", env!("CARGO_PKG_VERSION"), surface.name(), self.sock.peer_addr().map(|a| a.to_string()).unwrap_or_default());
        loop {
            let step_started = Instant::now();
            let stepped = self.step(surface);
            // Hardware pass: a step longer than the link's heartbeat interval delays every surface and the lease.
            let took = step_started.elapsed();
            if took >= Duration::from_millis(250) {
                eprintln!("loop: one step took {} ms; surfaces and heartbeats were blocked meanwhile", took.as_millis());
            }
            if let Err(e) = stepped {
                // The keypad went away: end every strip touch before the caller reopens the devices.
                let now = self.now();
                let outs = self.strips.disconnect();
                self.emit_strip_outs(outs, now);
                for o in self.banks.disconnect("mtouch") {
                    self.emit_bank_out(Some(o), now);
                }
                self.link.tick(now);
                for d in self.link.take_outgoing() {
                    let _ = self.sock.send(&d);
                }
                return Err(e);
            }
            if until(self, surface) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.bye_on_exit {
            self.link.bye();
            for d in self.link.take_outgoing() {
                let _ = self.sock.send(&d);
            }
        }
        Ok(())
    }
}

const ROTARIES: [&str; 4] = ["Rotary1", "Rotary2", "Rotary3", "Rotary4"];

/// Rotary `wheel` (1-4) means encoder slot `base + wheel - 1` of the bound display (KB-19 `--rotary-slots`).
fn link_target(wheel: u8, base: u8) -> protocol::CtlTarget {
    protocol::CtlTarget::Slot { slot: base.clamp(1, 5) + wheel.clamp(1, 4) - 1 }
}

/// KB-23 (review of PR #8): the lights are gated on the binding's readiness at the caller, not only in the renderer.
#[cfg(test)]
mod lights_binding_tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::VecDeque;

    struct Lamp {
        strips: VecDeque<mtouch::Event>,
        outputs: Vec<(mtouch::Model, mtouch::Output)>,
    }
    impl Surface for Lamp {
        fn poll(&mut self) -> Result<Vec<Event>> {
            Ok(Vec::new())
        }
        fn led(&mut self, _id: u16, _value: u16) {}
        fn name(&self) -> String {
            "lamp".into()
        }
        fn poll_strips(&mut self) -> Result<Vec<mtouch::Event>> {
            Ok(self.strips.pop_front().into_iter().collect())
        }
        fn device_present(&self, model: mtouch::Model) -> bool {
            model == mtouch::Model::MTouch
        }
        fn output(&mut self, model: mtouch::Model, out: mtouch::Output) {
            self.outputs.push((model, out));
        }
    }

    struct Plugin {
        seq: u64,
    }
    impl Plugin {
        fn frame(&mut self, mut v: Value) -> Vec<u8> {
            self.seq += 1;
            v["sid"] = json!("feedfacefeedface");
            v["seq"] = json!(self.seq);
            wire::frame(&v.to_string())
        }
        fn context(&mut self, br: u64, ex: Value, bind_ex: Vec<u32>) -> Vec<u8> {
            self.frame(json!({"t":"context","gen":"1-deadbeef","epoch":1,"known":1,"cg":1,"enc":{},"slots":[],"ex":ex,"page":1,"pool":"Default","display":1,"bind":{"ex":bind_ex,"br":br,"mode":"current"}}))
        }
    }

    fn key_writes(outs: &[(mtouch::Model, mtouch::Output)], id: u16) -> Vec<u16> {
        outs.iter().filter_map(|(_, o)| match o { mtouch::Output::LedKey { id: i, value } if *i == id => Some(*value), _ => None }).collect()
    }

    #[test]
    fn a_bank_change_lights_nothing_from_the_previous_bindings_context_until_the_new_one_arrives() {
        let cli = test_cli();
        let mut lp = Loop::new(&cli).expect("a loop on an ephemeral socket");
        lp.lights.cfg.min_interval = 0.0;
        let mut plugin = Plugin { seq: 0 };
        let mut lamp = Lamp { strips: VecDeque::new(), outputs: Vec::new() };
        // Pair: the hello goes out in the first step; the welcome answers its nonce.
        lp.step(&mut lamp).unwrap();
        let nonce = lp.link.hello_nonce().expect("a hello was sent");
        let welcome = json!({"t":"welcome","v":2,"sid":"feedfacefeedface","nonce":nonce,"gen":"1-deadbeef","lease":2000,"hb":250,"keys":{"ok":["Record"],"unsupported":[]},"modules":{},"input":"fake","epoch":1,"control":1,"controlBackend":"console","context":1,"seq":0});
        lp.link.receive(&wire::frame(&welcome.to_string()), lp.now());
        lp.step(&mut lamp).unwrap(); // the bank window's bind goes out
        let ev = lp.link.pending_bind_ev().expect("the bind is in flight");
        assert!(lp.link.lighting_context(lp.now()).is_none());
        lp.link.receive(&plugin.frame(json!({"t":"ack","ev":ev,"ok":1,"br":1})), lp.now());
        // The plugin reports the binding it was asked for (the whole bank-1 window); the feedback list names 101.
        let w1 = lp.banks.targets().ex;
        { let now = lp.now(); let m = plugin.context(1, json!([{"n":101,"tgt":1,"cls":"Sequence","name":"A","kp":"Go+","fd":"Master","lvl":100,"act":0,"col":"ff0000"}]), w1.clone()); lp.link.receive(&m, now); }
        lp.step(&mut lamp).unwrap();
        assert_eq!(key_writes(&lamp.outputs, 0x4201).last(), Some(&mtouch::LedKey::off().blue().value()), "PFA1 lit: executor 101 assigned ({:?})", lamp.outputs.len());
        assert!(lp.lights.lines.iter().any(|l| l.contains("PFA1 ex101") && l.contains("Assigned")), "{:?}", lp.lights.lines);
        // Our bank change: the strip now means 111; the bind for it is in flight. The old context (101 assigned) is
        // still the link's data, but it must not light PFA1 as 111's state.
        lamp.outputs.clear();
        lamp.strips.push_back(mtouch::Event::KeyDown { id: 0x4111, name: "PLAYBACK BANK Up (+)" });
        lp.step(&mut lamp).unwrap();
        assert!(lp.link.context_view(lp.now()).is_some(), "the previous context is still held as data");
        assert!(lp.link.lighting_context(lp.now()).is_none(), "but nothing lights from it while the bind is in flight");
        // Hardware pass: a rebind in flight is dark (not 101's assigned state, and not the stale pattern, which read as
        // an error during a normal bank change).
        assert_eq!(key_writes(&lamp.outputs, 0x4201), vec![0], "PFA1 goes dark, not 101's assigned state: {:?}", lamp.outputs);
        assert!(lp.lights.lines.iter().any(|l| l.contains("PFA1 ex111") && l.contains("rebinding")), "{:?}", lp.lights.lines);
        // The ack (revision 2) without its context: still unavailable, nothing rewritten.
        let ev = lp.link.pending_bind_ev().expect("the new bind is in flight");
        lp.link.receive(&plugin.frame(json!({"t":"ack","ev":ev,"ok":1,"br":2})), lp.now());
        lamp.outputs.clear();
        lp.step(&mut lamp).unwrap();
        assert!(lp.link.lighting_context(lp.now()).is_none());
        assert!(key_writes(&lamp.outputs, 0x4201).is_empty(), "{:?}", lamp.outputs);
        // The context of revision 2 reports the new (bank-2) binding and lists 111 empty: PFA1 goes dark; 101's red never reached the new mapping.
        let w2 = lp.banks.targets().ex;
        assert_ne!(w1, w2, "the bank change moved the bound window");
        { let now = lp.now(); let m = plugin.context(2, json!([{"n":111,"empty":1,"tgt":0}]), w2); lp.link.receive(&m, now); }
        lamp.outputs.clear();
        lp.step(&mut lamp).unwrap();
        assert!(key_writes(&lamp.outputs, 0x4201).is_empty(), "already dark from the rebind: nothing to rewrite: {:?}", lamp.outputs);
        assert!(lp.lights.lines.iter().any(|l| l.contains("PFA1 ex111: empty, dark")), "{:?}", lp.lights.lines);
    }

    #[test]
    fn review_10_round_2_a_context_that_moves_a_touched_strip_never_relabels_its_old_prediction() {
        // The strip stays touched while a context arrives that puts another attribute on its slot (generation 1 -> 2).
        // The old attribute's 90 % prediction must not be drawn for the new attribute, nor settle against it.
        // The loop's own UDP peer: the moved context is delivered over the socket so it arrives INSIDE a step (after
        // the step's gesture checks, before its render), as it does in production.
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").expect("a test peer");
        let cli = Cli::parse_from(["mtpnxk", "--plugin", &format!("127.0.0.1:{}", peer.local_addr().unwrap().port()), "--id", "mtpnxk-unit-test", "sim"]);
        let mut lp = Loop::new(&cli).expect("a loop on an ephemeral socket");
        lp.lights.cfg.min_interval = 0.0;
        let mut plugin = Plugin { seq: 0 };
        let mut lamp = Lamp { strips: VecDeque::new(), outputs: Vec::new() };
        lp.step(&mut lamp).unwrap();
        let nonce = lp.link.hello_nonce().expect("a hello was sent");
        let welcome = json!({"t":"welcome","v":2,"sid":"feedfacefeedface","nonce":nonce,"gen":"1-deadbeef","lease":2000,"hb":250,"keys":{"ok":["Record"],"unsupported":[]},"modules":{},"input":"fake","epoch":1,"control":1,"controlBackend":"console","context":1,"seq":0});
        lp.link.receive(&wire::frame(&welcome.to_string()), lp.now());
        lp.step(&mut lamp).unwrap();
        let ev = lp.link.pending_bind_ev().expect("the bind is in flight");
        lp.link.receive(&plugin.frame(json!({"t":"ack","ev":ev,"ok":1,"br":1})), lp.now());
        let w1 = lp.banks.targets().ex;
        let ctx = |plugin: &mut Plugin, cg: u64, name: &str, abs: f64| plugin.frame(json!({"t":"context","gen":"1-deadbeef","epoch":1,"known":1,"cg":cg,"enc":{},"ex":[],"page":1,"pool":"Default","display":1,
            "bind":{"ex":w1.clone(),"br":1,"mode":"current"},
            "slots":[{"n":1,"kind":"attribute","name":name,"readout":"Percent","res":"Coarse","avail":"available","val":"value","abs":abs,"vc":1}]}));
        { let now = lp.now(); let m = ctx(&mut plugin, 1, "ColorRGB_R", 90.0); lp.link.receive(&m, now); }
        lp.step(&mut lamp).unwrap();
        // Touch strip 1 and move it a little: the echo starts from 90 %.
        lamp.strips.push_back(mtouch::Event::Fader { id: 0x6112, name: "VALUE BASE CHANNEL 1", value: 100, touch: true });
        lp.step(&mut lamp).unwrap();
        lamp.strips.push_back(mtouch::Event::Fader { id: 0x6112, name: "VALUE BASE CHANNEL 1", value: 102, touch: true });
        lp.step(&mut lamp).unwrap();
        assert!(lp.strips.echo(1).is_some(), "the touched strip predicts");
        // While still touched, a context puts Dimmer on slot 1 at 20 % (generation 2), received before the render.
        lamp.outputs.clear();
        let m = ctx(&mut plugin, 2, "Dimmer", 20.0);
        peer.send_to(&m, lp.sock.local_addr().unwrap()).expect("sent to the loop");
        std::thread::sleep(Duration::from_millis(5));
        lp.step(&mut lamp).unwrap();
        assert_eq!(lp.link.current_generation(lp.now()), Some(2), "the moved context arrived inside the step");
        let bar = |outs: &[(mtouch::Model, mtouch::Output)]| outs.iter().rev().find_map(|(_, o)| match o { mtouch::Output::FaderBar { id: 0x6112, data } => Some(*data), _ => None });
        let twenty = mtouch::bar_level(lights::leds_of(0.20), mtouch::BarColour::Blue, false);
        assert_eq!(bar(&lamp.outputs), Some(twenty), "the new attribute's 20 %, not the old 90 %: {:?}", lamp.outputs);
        // The next steps (still touched, then lifted) never bring the old prediction back as a settling value.
        lamp.outputs.clear();
        lp.step(&mut lamp).unwrap();
        lamp.strips.push_back(mtouch::Event::Fader { id: 0x6112, name: "VALUE BASE CHANNEL 1", value: 102, touch: false });
        lp.step(&mut lamp).unwrap();
        lp.step(&mut lamp).unwrap();
        assert!(bar(&lamp.outputs).map(|b| b == twenty).unwrap_or(true), "no 90 % after the context moved: {:?}", lamp.outputs);
        assert_eq!(lp.lights.want_bar(mtouch::Model::MTouch, 0x6112), Some(twenty));
    }

    /// Review of PR #11: a paired loop whose context lists the given executors (helper for the timing tests).
    fn paired_with_executors(ex: Value) -> (Loop, Plugin, Lamp) {
        let cli = test_cli();
        let mut lp = Loop::new(&cli).expect("a loop on an ephemeral socket");
        let mut plugin = Plugin { seq: 0 };
        let mut lamp = Lamp { strips: VecDeque::new(), outputs: Vec::new() };
        lp.step(&mut lamp).unwrap();
        let nonce = lp.link.hello_nonce().expect("a hello was sent");
        let welcome = json!({"t":"welcome","v":2,"sid":"feedfacefeedface","nonce":nonce,"gen":"1-deadbeef","lease":2000,"hb":250,"keys":{"ok":["Record"],"unsupported":[]},"modules":{},"input":"fake","epoch":1,"control":1,"controlBackend":"console","context":1,"seq":0});
        lp.link.receive(&wire::frame(&welcome.to_string()), lp.now());
        lp.step(&mut lamp).unwrap();
        let ev = lp.link.pending_bind_ev().expect("the bind is in flight");
        lp.link.receive(&plugin.frame(json!({"t":"ack","ev":ev,"ok":1,"br":1})), lp.now());
        let w = lp.banks.targets().ex;
        { let now = lp.now(); let m = plugin.context(1, ex, w); lp.link.receive(&m, now); }
        (lp, plugin, lamp)
    }

    #[test]
    fn review_11_unreadable_activity_keeps_the_timing_and_only_a_left_executor_cancels_it() {
        // Bound, activity "?": the timing keeps waiting, then times the readable change.
        let (mut lp, mut plugin, _lamp) = paired_with_executors(json!([{"n":101,"tgt":1,"cls":"Sequence","name":"A","kp":"Flash","fd":"Master","lvl":0,"act":0}]));
        let now = lp.now();
        lp.note_key_timing(101, None, true, "MPA1", now);
        assert!(lp.key_timing.contains_key(&(101, None)), "a down on an idle executor is timed");
        let w = lp.banks.targets().ex;
        { let m = plugin.context(1, json!([{"n":101,"tgt":1,"cls":"Sequence","name":"A","kp":"Flash","fd":"Master","lvl":0,"act":"?"}]), w.clone()); lp.link.receive(&m, now + 0.01); }
        lp.time_playback_keys(now + 0.02);
        assert!(lp.key_timing.contains_key(&(101, None)), "still bound with unreadable activity: not cancelled as unbound");
        assert!(lp.key_effect_ms.is_empty());
        { let m = plugin.context(1, json!([{"n":101,"tgt":1,"cls":"Sequence","name":"A","kp":"Flash","fd":"Master","lvl":0,"act":1}]), w.clone()); lp.link.receive(&m, now + 0.05); }
        lp.time_playback_keys(now + 0.06);
        assert!(!lp.key_timing.contains_key(&(101, None)));
        assert_eq!(lp.key_effect_ms.len(), 1, "the readable change is timed");
        // Left the binding: cancelled, nothing timed.
        let (mut lq, mut pq, _lq_lamp) = paired_with_executors(json!([{"n":101,"tgt":1,"cls":"Sequence","name":"A","kp":"Flash","fd":"Master","lvl":0,"act":0}]));
        let t = lq.now();
        lq.note_key_timing(101, None, true, "MPA1", t);
        let wq = lq.banks.targets().ex;
        { let m = pq.context(1, json!([]), wq); lq.link.receive(&m, t + 0.01); }
        lq.time_playback_keys(t + 0.02);
        assert!(!lq.key_timing.contains_key(&(101, None)), "the executor left the binding: cancelled");
        assert!(lq.key_effect_ms.is_empty(), "and not timed");
    }

    #[test]
    fn a_refused_bank_change_keeps_the_lights_unavailable_not_the_old_context() {
        // Review of PR #8 (round 2): when the plugin refuses the new binding, the surface still maps the new
        // bank, so the previous binding's context must not light the controls (that would show the new bank's
        // controls with the old bank's feedback, or as empty).
        let cli = test_cli();
        let mut lp = Loop::new(&cli).expect("a loop on an ephemeral socket");
        lp.lights.cfg.min_interval = 0.0;
        let mut plugin = Plugin { seq: 0 };
        let mut lamp = Lamp { strips: VecDeque::new(), outputs: Vec::new() };
        lp.step(&mut lamp).unwrap();
        let nonce = lp.link.hello_nonce().expect("a hello was sent");
        let welcome = json!({"t":"welcome","v":2,"sid":"feedfacefeedface","nonce":nonce,"gen":"1-deadbeef","lease":2000,"hb":250,"keys":{"ok":["Record"],"unsupported":[]},"modules":{},"input":"fake","epoch":1,"control":1,"controlBackend":"console","context":1,"seq":0});
        lp.link.receive(&wire::frame(&welcome.to_string()), lp.now());
        lp.step(&mut lamp).unwrap();
        let ev = lp.link.pending_bind_ev().expect("the bind is in flight");
        lp.link.receive(&plugin.frame(json!({"t":"ack","ev":ev,"ok":1,"br":1})), lp.now());
        let w1 = lp.banks.targets().ex;
        { let now = lp.now(); let m = plugin.context(1, json!([{"n":101,"tgt":1,"cls":"Sequence","name":"A","kp":"Go+","fd":"Master","lvl":100,"act":0,"col":"ff0000"}]), w1.clone()); lp.link.receive(&m, now); }
        lp.step(&mut lamp).unwrap();
        assert_eq!(key_writes(&lamp.outputs, 0x4201).last(), Some(&mtouch::LedKey::off().blue().value()), "PFA1 lit for 101");
        // Bank change: PFA1 now maps to 111; the bind for it goes out.
        lamp.outputs.clear();
        lamp.strips.push_back(mtouch::Event::KeyDown { id: 0x4111, name: "PLAYBACK BANK Up (+)" });
        lp.step(&mut lamp).unwrap();
        let ev = lp.link.pending_bind_ev().expect("the new bind is in flight");
        assert_eq!(key_writes(&lamp.outputs, 0x4201), vec![0], "dark while in flight (the rebind grace)");
        // The plugin REFUSES the new binding and keeps [101..110]; it resends its unchanged context.
        lamp.outputs.clear();
        lp.link.receive(&plugin.frame(json!({"t":"ack","ev":ev,"ok":0,"code":"bad-bind","why":"cannot watch in full"})), lp.now());
        // The plugin kept its bank-1 binding (w1); the surface already maps bank 2.
        { let now = lp.now(); let m = plugin.context(1, json!([{"n":101,"tgt":1,"cls":"Sequence","name":"A","kp":"Go+","fd":"Master","lvl":100,"act":0,"col":"ff0000"}]), w1.clone()); lp.link.receive(&m, now); }
        lp.step(&mut lamp).unwrap();
        assert!(lp.link.lighting_context(lp.now()).is_none(), "the refused bind must not re-enable lighting from the old context");
        assert!(key_writes(&lamp.outputs, 0x4201).is_empty(), "still dark inside the grace: {:?}", lamp.outputs);
        // A refused bind never resolves: past the grace it is staleness and shows the unavailable pattern.
        lp.lights.age_rebind(lights::REBIND_GRACE + 0.1);
        lamp.outputs.clear();
        lp.step(&mut lamp).unwrap();
        assert!(key_writes(&lamp.outputs, 0x4201).iter().all(|&v| v != mtouch::LedKey::off().blue().value() && v != 0), "PFA1 is neither 101's assigned nor dark: {:?}", lamp.outputs);
        assert!(lp.lights.lines.iter().any(|l| l.contains("PFA1 ex111") && l.contains("unavailable")), "PFA1 is shown unavailable, mapped to 111: {:?}", lp.lights.lines);
    }
}

#[cfg(test)]
mod keypad_loss_tests {
    use super::*;
    use std::collections::VecDeque;

    /// A surface that plays keypad events, then reports the keypad lost, then strip reports.
    struct Script {
        keys: VecDeque<Event>,
        /// The keypad is found gone on this many-th empty poll after the last key (None: never).
        lose_on_empty_poll: Option<usize>,
        empty_polls: usize,
        pending_loss: bool,
        strips: VecDeque<mtouch::Event>,
        /// KB-21 (review): the M-Touch is found gone on the poll after its last report.
        lose_mtouch: bool,
        mplay: VecDeque<mtouch::Event>,
    }
    impl Surface for Script {
        /// One key per poll; strips are played one report per poll once the keys are done.
        fn poll(&mut self) -> Result<Vec<Event>> {
            match self.keys.pop_front() {
                Some(ev) => Ok(vec![ev]),
                None => {
                    self.empty_polls += 1;
                    if self.lose_on_empty_poll == Some(self.empty_polls) {
                        self.pending_loss = true;
                    }
                    Ok(Vec::new())
                }
            }
        }
        fn led(&mut self, _id: u16, _value: u16) {}
        fn name(&self) -> String {
            "script".into()
        }
        fn disconnected(&mut self) -> bool {
            std::mem::take(&mut self.pending_loss)
        }
        fn poll_strips(&mut self) -> Result<Vec<mtouch::Event>> {
            if !self.keys.is_empty() { return Ok(Vec::new()); }
            match self.strips.pop_front() {
                Some(ev) => Ok(vec![ev]),
                None if self.lose_mtouch => { self.lose_mtouch = false; Err(anyhow!("M-Touch unplugged")) }
                None => Ok(Vec::new()),
            }
        }
        fn poll_mplay(&mut self) -> Result<Vec<mtouch::Event>> {
            Ok(self.mplay.pop_front().into_iter().collect())
        }
        fn device_present(&self, _model: mtouch::Model) -> bool {
            true
        }
    }

    fn test_loop() -> Loop {
        let cli = test_cli();
        Loop::new(&cli).expect("a loop on an ephemeral socket")
    }
    fn strip1(value: u8, touch: bool) -> mtouch::Event {
        mtouch::Event::Fader { id: 0x6112, name: "VALUE BASE CHANNEL 1", value, touch }
    }

    #[test]
    fn losing_the_keypad_releases_its_keys_pushes_and_bank_but_not_the_strips() {
        let mut lp = test_loop();
        let mut s = Script {
            keys: VecDeque::from([Event::KeyDown { name: "Clear", id: 1 }, Event::KeyDown { name: "Bank", id: 2 }, Event::PressDown { wheel: 2, id: 0 }]),
            lose_on_empty_poll: Some(3),
            empty_polls: 0,
            pending_loss: false,
            // Strip reports from the fourth step on: a touch and a move with Bank still held, then (after the loss) a move.
            strips: VecDeque::from([strip1(0, true), strip1(20, true), strip1(51, true), strip1(80, true)]),
            lose_mtouch: false,
            mplay: VecDeque::new(),
        };
        for _ in 0..3 { lp.step(&mut s).unwrap(); }
        assert!(lp.link.held().contains("Clear") && lp.link.held().contains("Bank"), "{:?}", lp.link.held());
        assert!(lp.bank_held && lp.pushed_wheels.contains(&2));
        lp.step(&mut s).unwrap();  // strip touch
        lp.step(&mut s).unwrap();  // strip move with Bank held: fine
        assert_eq!(lp.last_strip_fine, Some(true), "before the loss Bank makes the strips fine");
        assert!(lp.strips.touching(1));
        lp.step(&mut s).unwrap();  // the keypad is found gone in this poll: releases, then the strip move
        assert!(lp.link.held().is_empty(), "every key the link held for the keypad is released: {:?}", lp.link.held());
        assert!(!lp.bank_held, "the Bank modifier is cleared");
        assert!(lp.pushed_wheels.is_empty(), "the rotary push is ended");
        // Unpaired, the releases are counted as dropped (nothing to send them to) rather than lost.
        assert!(lp.link.stats.dropped_unpaired >= 3);
        assert_eq!(lp.last_strip_fine, Some(false), "the strip move right after the loss is no longer fine");
        assert!(lp.strips.touching(1), "the finger on the strip is still there: the M-Touch's gesture survives the keypad's loss");
        lp.step(&mut s).unwrap();
        assert_eq!(lp.last_strip_fine, Some(false));
        assert_eq!(lp.strips.stats.touches, 1);
        assert_eq!(lp.strips.stats.forced_lifts, 0, "the strip was not released by the keypad's loss");
        // A loss with nothing held releases nothing and changes nothing.
        let mut lp2 = test_loop();
        let mut s2 = Script { keys: VecDeque::new(), lose_on_empty_poll: Some(1), empty_polls: 0, pending_loss: false, strips: VecDeque::new(), lose_mtouch: false, mplay: VecDeque::new() };
        lp2.step(&mut s2).unwrap();
        assert!(lp2.link.held().is_empty() && !lp2.bank_held && lp2.link.stats.dropped_unpaired == 0);
    }

    /// Review (PR #6): losing the M-Touch releases its held playback buttons against their frozen targets; an M-Play's hold stays.
    #[test]
    fn losing_the_mtouch_releases_its_playback_buttons_but_not_the_mplays() {
        let mut lp = test_loop();
        let pfa1 = |down: bool| if down { mtouch::Event::KeyDown { id: 0x4201, name: "PFA 1" } } else { mtouch::Event::KeyUp { id: 0x4201, name: "PFA 1" } };
        let mut s = Script {
            keys: VecDeque::new(), lose_on_empty_poll: None, empty_polls: 0, pending_loss: false,
            strips: VecDeque::from([mtouch::Event::KeyDown { id: 0x4111, name: "PLAYBACK BANK Up (+)" }, pfa1(true), strip1(10, true)]),
            lose_mtouch: true,
            mplay: VecDeque::from([mtouch::Event::KeyDown { id: 0x9205, name: "FADER PFD 1" }]),
        };
        lp.step(&mut s).unwrap();  // bank +: the M-Touch section on bank 2; the M-Play's top button 1 down
        lp.step(&mut s).unwrap();  // PFA 1 down on bank 2: executor 111 key
        lp.step(&mut s).unwrap();  // a parameter strip touch
        assert_eq!(lp.banks.held_count(), 2, "one M-Touch and one M-Play button held");
        assert!(lp.strips.touching(1));
        lp.banks.key(0x4111, false);
        lp.step(&mut s).unwrap();  // the M-Touch is found gone
        assert!(!lp.strips.touching(1), "the strip touch is ended");
        assert_eq!(lp.banks.held_count(), 1, "the M-Touch's playback button is released, the M-Play's stays held");
        assert!(lp.banks.log.iter().any(|l| l.contains("mtouch/PFA1") && l.contains("lost while held") && l.contains("executor 111 key")), "{:?}", lp.banks.log);
        assert_eq!(lp.link.stats.dropped_unpaired, 5, "bank-less: PFA1 down, MPA1 down, touch, the strip's forced lift and the frozen release all reached the link (unpaired, counted)");
    }
}

#[cfg(test)]
mod rotary_tests {
    use super::*;
    #[test]
    fn rotaries_map_to_a_slot_window() {
        assert_eq!(link_target(1, 1), protocol::CtlTarget::Slot { slot: 1 });
        assert_eq!(link_target(4, 1), protocol::CtlTarget::Slot { slot: 4 });
        assert_eq!(link_target(1, 2), protocol::CtlTarget::Slot { slot: 2 });
        assert_eq!(link_target(4, 2), protocol::CtlTarget::Slot { slot: 5 });
        assert_eq!(link_target(9, 1), protocol::CtlTarget::Slot { slot: 4 }, "an out-of-range wheel clamps");
        assert_eq!(link_target(4, 9), protocol::CtlTarget::Slot { slot: 8 }, "the base is bounded so the slot stays within the plugin's 1..8");
    }
}

fn percentiles(label: &str, samples: &[f64]) {
    if samples.is_empty() {
        println!("{label}: no samples");
        return;
    }
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| s[((s.len() - 1) as f64 * q).round() as usize];
    println!("{label}: n={} median={:.1} p95={:.1} p99={:.1} worst={:.1} ms", s.len(), p(0.5), p(0.95), p(0.99), s[s.len() - 1]);
}

/// Hardware-pass finding (2026-10-11): unit tests that build the real loop used the default `--plugin`
/// (127.0.0.1:9810), so `cargo test` sent hellos as `nxk-host` to a live plugin and replaced the running service's
/// session ("replaced-by-hello"). Tests point at a local sink that nobody answers.
#[cfg(test)]
fn test_cli() -> Cli {
    let sink = std::net::UdpSocket::bind("127.0.0.1:0").expect("binding a test sink");
    let port = sink.local_addr().expect("sink address").port();
    std::mem::forget(sink);
    Cli::parse_from(["mtpnxk", "--plugin", &format!("127.0.0.1:{port}"), "--id", "mtpnxk-unit-test", "sim"])
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::List => {
            for line in nxk::usb::list()? {
                println!("{line}");
            }
            Ok(())
        }
        Cmd::MtouchListen { seconds, pid } => mtouch::tools::listen(mtouch_model(pid.as_deref())?, *seconds),
        Cmd::MtouchLedTest { pid, hold_ms } => mtouch::tools::led_test(mtouch_model(pid.as_deref())?, Duration::from_millis(*hold_ms)),
        Cmd::Run => {
            let mut lp = Loop::new(&cli)?;
            let with_mtouch = match cli.mtouch.to_ascii_lowercase().as_str() {
                "auto" => true,
                "off" | "none" => false,
                other => return Err(anyhow!("--mtouch {other}: expected auto or off")),
            };
            // KB-20 (review of PR #5): the NX-K and the M-Touch are opened and reopened independently; the loop runs
            // with whichever is present (an M-Touch alone polls its strips; losing the NX-K does not interrupt it).
            // KB-22: the M-Touch is wanted for its playback strips even with the parameter strips off.
            let open_mtouch: Option<Box<dyn FnMut() -> Result<mtouch::usb::UsbSurface>>> =
                if with_mtouch && (cli.strips != strips::Mode::Off || lp.banks.cfg.enabled) { Some(Box::new(|| mtouch::usb::UsbSurface::open_model(mtouch::Model::MTouch))) } else { None };
            // KB-21: the M-Play is a third independently reopened device (its playback buttons and bank keys).
            let open_mplay: Option<Box<dyn FnMut() -> Result<mtouch::usb::UsbSurface>>> =
                if lp.banks.cfg.mplay && lp.banks.cfg.enabled { Some(Box::new(|| mtouch::usb::UsbSurface::open_model(mtouch::Model::MPlay))) } else { None };
            let mut surface = usb_surfaces::Combined::new(Box::new(nxk::usb::UsbKeypad::open), open_mtouch).with_mplay(open_mplay);
            loop {
                let r = lp.run(&mut surface, |_, _| false);
                eprintln!("surface loop ended: {r:?}; continuing in 1 s");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Cmd::Sim { script, repeat, seconds, abandon } => {
            let steps = sim::parse_script(script)?;
            let mut keypad = sim::SimKeypad::new(steps, *repeat, true);
            let mut lp = Loop::new(&cli)?;
            let deadline = if *seconds > 0.0 { Some(Duration::from_secs_f64(*seconds)) } else { None };
            lp.bye_on_exit = !abandon;
            lp.run(&mut keypad, |lp, s| {
                let timed_out = deadline.map(|d| lp.started.elapsed() > d).unwrap_or(false);
                // After the script ends, give acks a moment to arrive.
                let finished = !repeat && s.done() && lp.started.elapsed() > Duration::from_millis(500) && lp.link.pending_count() == 0;
                timed_out || finished
            })?;
            print_summary(&lp.link, &lp.strips);
            println!("{}", lp.banks.summary(lp.link.console_page(lp.now())));
            // KB-23: the textual state of every light (colour is never the only cue) and the renderer's counters.
            println!("{}", lp.lights.summary());
            for l in &lp.lights.lines {
                println!("light: {l}");
            }
            Ok(())
        }
        Cmd::Measure { seconds, key_rate, motion_rate, probe_rate, probe_key, probe_item, no_strip, no_playback, json } => {
            let probe = sim::key_name(probe_key)?;
            let script = measure::workload(&measure::Workload { key_rate: *key_rate, motion_rate: *motion_rate, probe_rate: *probe_rate, probe_key: probe, strip: !no_strip, playback: !no_playback });
            let mut lp = Loop::new(&cli)?;
            lp.probe = Some((probe, probe_item.clone()));
            // Review (PR #9): one session for warm-up, workload and drain; only the drain says bye.
            lp.bye_on_exit = false;
            // Warm-up: pair and wait for a known binding generation (the context), at most 5 s, with no input.
            let mut idle = sim::SimKeypad::new(Vec::new(), false, false);
            lp.run(&mut idle, |lp, _| lp.link.current_generation(lp.now()).is_some() || lp.started.elapsed() > Duration::from_secs(5))?;
            let warm = lp.now();
            let paired = lp.link.session().is_some();
            let context = lp.link.current_generation(warm).is_some();
            let before = measure::Snapshot::of(&lp.link, &lp.strips, &lp.banks);
            let mut keypad = sim::SimKeypad::new(sim::parse_script(&script)?, true, false);
            let total = Duration::from_secs_f64(*seconds);
            let start = Instant::now();
            lp.run(&mut keypad, |_, _| start.elapsed() > total)?;
            // Drain: let the last acks and states arrive, then say bye.
            let mut idle = sim::SimKeypad::new(Vec::new(), false, false);
            let drain_from = Instant::now();
            lp.bye_on_exit = true;
            lp.run(&mut idle, |lp, _| drain_from.elapsed() > Duration::from_millis(600) && lp.link.pending_count() == 0 || drain_from.elapsed() > Duration::from_secs(3))?;
            let report = measure::report(&lp.link, &lp.strips, &lp.banks, &before, measure::Setup { seconds: *seconds, key_rate: *key_rate, motion_rate: *motion_rate, probe_rate: *probe_rate, probe_key: probe.to_string(), probe_item: probe_item.clone(), strip: !no_strip, playback: !no_playback, plugin: cli.plugin.clone(), paired, context_at_start: context, warmup_s: warm });
            print!("{}", report.text());
            print_summary(&lp.link, &lp.strips);
            println!("{}", lp.banks.summary(lp.link.console_page(lp.now())));
            if let Some(path) = json {
                std::fs::write(path, serde_json::to_string_pretty(&report.json())?).with_context(|| format!("writing {}", path.display()))?;
                eprintln!("measure: report written to {}", path.display());
            }
            if report.failed() { std::process::exit(2); }
            Ok(())
        }
        Cmd::Bench { taps, rate } => {
            let gap = (1000.0 / rate).max(1.0) as u64;
            let mut script = String::new();
            for i in 0..*taps {
                if i % 10 == 9 {
                    script.push_str(&format!("Clear:tap@{gap},"));
                } else {
                    script.push_str(&format!("5:tap@{gap},"));
                }
            }
            let steps = sim::parse_script(&script)?;
            let total = Duration::from_millis(gap * (*taps as u64 + 2) + 1500);
            let mut keypad = sim::SimKeypad::new(steps, false, false);
            let mut lp = Loop::new(&cli)?;
            lp.run(&mut keypad, |lp, _| lp.started.elapsed() > total)?;
            println!("bench: {} taps at {rate}/s against {}", taps, cli.plugin);
            print_summary(&lp.link, &lp.strips);
            percentiles("ack round trip", &lp.link.rtt_ms);
            let effects: Vec<f64> = lp.link.effect_ms.iter().filter(|m| **m >= 0).map(|m| *m as f64).collect();
            let no_effect = lp.link.effect_ms.iter().filter(|m| **m < 0).count();
            if lp.link.effect_ms.is_empty() {
                println!("press-to-effect: no samples (start the plugin with `bench` to measure it)");
            } else {
                percentiles("press-to-effect (NUM taps, command line)", &effects);
                println!("press-to-effect: {no_effect} tap(s) showed no command-line change within the plugin's window");
            }
            Ok(())
        }
    }
}

/// `--pid f808|f80c` (with or without 0x) to a model; `None` means the first found.
fn mtouch_model(pid: Option<&str>) -> Result<Option<mtouch::Model>> {
    let Some(p) = pid else { return Ok(None) };
    let n = u16::from_str_radix(p.trim_start_matches("0x"), 16).with_context(|| format!("parsing --pid {p}"))?;
    mtouch::Model::from_pid(n).map(Some).ok_or_else(|| anyhow!("--pid {p}: not an M-Touch (f808) or M-Play (f80c)"))
}

fn print_summary(link: &link::Link, strips: &strips::Strips) {
    let st = &link.stats;
    println!(
        "link: paired={} hellos={} events={} acked={} refused={} retransmitted={} lost={} superseded={} states={} link_downs={} rejected={} dropped_unpaired={}",
        link.session().is_some(),
        st.hellos,
        st.events,
        st.acked,
        st.refused,
        st.retransmitted,
        st.lost,
        st.superseded,
        st.states,
        st.link_downs,
        st.rejected,
        st.dropped_unpaired
    );
    println!(
        "control: events={} sent={} coalesced={} unbound={} aged={} stale={} unsupported={} refused={} lost_reported={} superseded={} overflow={} queued={} max_detent={}",
        st.ctl_events, st.ctl_sent, st.ctl_coalesced, st.ctl_unbound, st.ctl_aged, st.ctl_stale, st.ctl_unsupported, st.ctl_refused, st.ctl_lost_reported, st.ctl_superseded, st.ctl_overflow, link.queued_motion(), st.ctl_max_detent
    );
    let ss = &strips.stats;
    if ss.reports > 0 || strips.cfg.mode != strips::Mode::Off {
        println!(
            "strips: mode={:?} reports={} touches={} lifts={} detents={} positions={} rebound_dropped={} forced_lifts={} waiting_pickup={} takeovers={} ignored={}",
            strips.cfg.mode, ss.reports, ss.touches, ss.lifts, ss.detents, ss.positions, ss.rebound_dropped, ss.forced_lifts, ss.waiting_pickup, ss.takeovers, ss.ignored
        );
    }
    if let Some(s) = link.session() {
        println!("session: sid={} plugin gen={} input={:?} control={} keys ok={} unsupported={:?}", s.sid, s.plugin_gen, s.input, if s.control { s.control_backend.clone().unwrap_or_else(|| "on".into()) } else { "off".into() }, s.keys.ok.len(), s.keys.unsupported);
    }
    for r in &link.refusals {
        println!("refused ev {}: [{}] {}", r.ev, r.code, r.why);
    }
}

#[cfg(test)]
mod local_transport_tests {
    use super::*;
    #[test]
    fn only_fixed_local_endpoints_are_accepted() {
        for endpoint in ["127.0.0.1:9810", "localhost:9811"] {
            assert_eq!(*plugin_address(endpoint).unwrap().ip(), Ipv4Addr::LOCALHOST);
        }
        for endpoint in ["0.0.0.0:9810", "192.168.1.20:9810", "example.com:9810", "[::1]:9810", "127.0.0.1:0", "localhost:65536"] {
            assert!(plugin_address(endpoint).is_err(), "accepted {endpoint}");
        }
        for args in [vec!["mtpnxk", "--key", "abc", "sim"], vec!["mtpnxk", "--key-file", "secret", "sim"], vec!["mtpnxk", "keygen"]] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        let cli = test_cli();
        let lp = Loop::new(&cli).unwrap();
        assert_eq!(lp.sock.local_addr().unwrap().ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(lp.sock.peer_addr().unwrap().ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(lp.sock.peer_addr().unwrap().port(), 9810, "tests never address the default plugin port");
    }
}
