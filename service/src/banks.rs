//! KB-21: playback banks and stable executor bindings, the surface half.
//!
//! The M-Touch's ten playback strips (`PFA n` / `PFB n` keys at 0x42n1 / 0x42n2, the fader at 0x42n3) and the
//! M-Play's two sections (the left block: twelve faders with a top and a bottom button each, `FADER PFD 1-12` /
//! `13-24`; the right block: twenty-four `FADER EXT PF` buttons) operate assigned grandMA3 executors. Which
//! executor a physical control means is a PROFILE decision made here, never inferred from the console:
//!
//!   * an EXECUTOR PAGE is the console's (the user's current page, or one named explicitly); a SURFACE BANK is a
//!     window of this surface's controls on that page. Bank `k` (0-based) of a section of `size` strips maps
//!     strip `i` (1-based) to executor `row + k x size + (i - 1)` on each of the section's rows: the M-Touch's
//!     and the M-Play left block's strips have three rows (`--pb-rows <keyA>,<fader>,<keyB>`, default
//!     `101,201,301`: the console's default layout of a button above, the fader's own executor and a button
//!     below), the M-Play's right block one (`--pb-ext-row`, default 401). A bank never leaves its row's
//!     hundred block (`k <= 90 / size - 1`): the bank keys stop at the edge, counted and logged.
//!   * FOLLOW mode (`--pb-page follow`, the default) binds the executors without a page: the plugin reads them on
//!     the console's current page and the targets follow a `Page n` on the console (the binding generation
//!     moves; nothing here issues a page command). INDEPENDENT mode (`--pb-page <n>`) binds them to that page:
//!     read through `ObjectList("Page P.E")` whatever page the console shows, the generation ignores the console's
//!     page changes, and a page that does not exist is reported `pageMissing` by the plugin and never created.
//!     Local banking changes the bound executor list only (`bind`), never the console's page.
//!   * a pressed button's target is FROZEN until its release: the release carries the target the down was sent
//!     with, whatever bank the section is on by then (the plugin freezes the resolved target on its side too).
//!     A bank change while a button is held therefore neither releases nor activates the newly mapped executor;
//!     the new mapping is reached by a fresh press only.
//!   * KB-22: a playback FADER (M-Touch `LEVEL PLAYBACK n` 0x42n3, M-Play `FADER LEVEL n` 0x92n3) operates the
//!     fader row's executor through its `fader` element: the touch-down is a hold on the executor the strip means
//!     right then (FROZEN, like a button), positions go out only after PICKUP - the strip crossed the executor's
//!     last known level from the plugin's context (`lvl`, the configured fader function's level) or landed within
//!     `pickup_tolerance` of it - and the lift ends the hold. There is no takeover modifier for playback faders:
//!     an executor whose level is unknown (not a playback target, no fader function, no context) waits, counted.
//!     Pickup starts over on every touch, after a bank change or a plugin rebind (the touch is rebound: motion
//!     dropped until the lift), after a reconnect (a lost device ends its touches) and after an EXTERNAL EDIT:
//!     a context received more than `external_grace` after our last position that reports the level away from
//!     it (beyond twice the tolerance) means someone else moved the fader on the console; the strip must cross
//!     the new level again. A device's startup or resting reports (`touch` 0 while nothing is touched) seed
//!     nothing: no level is ever initialised from the device. The console backend honours the executor's
//!     CONFIGURED fader function (Master, Temp; others refused until qualified) and scales the position over its
//!     travel; the service sends 0..1 of the strip and never assumes a function.
//!   * KB-22: while a touched fader waits for pickup, its RGB bar shows the span between the strip and the
//!     executor's level (yellow, blinking): the direction to move. The bar is cleared on pickup, lift and loss;
//!     level, activity and assignment colours are KB-23's.
//!   * KB-22: buttons go out as `btn` events on the executor's `key` element and the console backend dispatches
//!     the executor's CONFIGURED button function (Press/Unpress Page P.E); the M-Play's playback buttons are
//!     PRESSURE keys (kind V) on the hardware: a report with `touch` 1 is the down, the release report (`touch` 0)
//!     the up, the pressure value itself is never an input.
//!   * KB-22: the dedicated keys are mapped only through an EXPLICIT profile: `--pb-go <executor>` makes the
//!     `Next (Go)` key (0x5513, both devices) press that executor's `key` element (its configured function,
//!     whatever it is: nothing here assumes Go+), `--pb-pause <executor>` the same for `Pause (||/Back)`
//!     (0x5512); the executor is on the bound page (follow or independent) and never banked. Without the flag
//!     the keys stay unmapped and counted; SELECT, RELEASE, BEAT and SNAP are ambiguous and stay unmapped.
//!   * Indicators: the M-Touch page display (0x4401) and the M-Play's two displays (0x9F01 left, 0x9E01 right)
//!     show "page then bank" as `page x 10 + bank` (page 1 bank 2 = `12`; the page is the console's in follow
//!     mode, the fixed one otherwise) while both fit three digits, the bank alone otherwise, blank while the
//!     page is unknown (no context). The mode is in the service's status line.
//!
//! The executor layout itself (an expanded assignment covering its neighbours, an empty or reserved number, a
//! Quickey) is the plugin's: a target the console reports covered, empty, reserved or on a missing page is refused
//! by the plugin with the reason, and this module maps controls to numbers only.

use crate::link::{BindSpec, CtlKind};
use crate::protocol::CtlTarget;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMode {
    Follow,
    Page(u32),
}

impl std::str::FromStr for PageMode {
    type Err = String;
    fn from_str(s: &str) -> Result<PageMode, String> {
        match s.to_ascii_lowercase().as_str() {
            "follow" | "console" => Ok(PageMode::Follow),
            other => other.parse::<u32>().ok().filter(|n| (1..=9999).contains(n)).map(PageMode::Page).ok_or_else(|| format!("--pb-page {other}: expected follow or a page number 1..9999")),
        }
    }
}

/// The three executor rows a strip section maps to (hundred-block bases).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rows {
    pub key_a: u32,
    pub fader: u32,
    pub key_b: u32,
}

impl std::str::FromStr for Rows {
    type Err = String;
    fn from_str(s: &str) -> Result<Rows, String> {
        let parts: Vec<u32> = s.split(',').map(|p| p.trim().parse::<u32>().map_err(|_| format!("--pb-rows {s}: expected three executor row bases like 101,201,301"))).collect::<Result<_, _>>()?;
        if parts.len() != 3 || parts.iter().any(|b| !(1..=999).contains(b) || b % 100 != 1) {
            return Err(format!("--pb-rows {s}: expected three row bases ending in 01 (101,201,301)"));
        }
        Ok(Rows { key_a: parts[0], fader: parts[1], key_b: parts[2] })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    KeyA,
    Fader,
    KeyB,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    /// Strips with a button above, a fader and a button below (M-Touch playback strips, M-Play left block).
    Strips(Rows),
    /// Buttons only, one row (M-Play right block).
    Keys(u32),
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: &'static str,
    pub dev: &'static str,
    pub model: crate::mtouch::Model,
    pub display: u16,
    pub size: u32,
    pub bank: u32,
    pub enabled: bool,
    /// Whether the section's device is attached right now (set by the loop); only present sections are bound.
    pub present: bool,
    pub kind: SectionKind,
}

impl Section {
    /// The highest bank index whose window still fits in the row's hundred block (executors `x01..x90`).
    pub fn max_bank(&self) -> u32 {
        (90 / self.size).saturating_sub(1)
    }
    fn row_base(&self, row: Row) -> u32 {
        match (self.kind, row) {
            (SectionKind::Strips(r), Row::KeyA) => r.key_a,
            (SectionKind::Strips(r), Row::Fader) => r.fader,
            (SectionKind::Strips(r), Row::KeyB) => r.key_b,
            (SectionKind::Keys(b), _) => b,
        }
    }
    /// Executor number of strip `i` (1-based) on `row` at the current bank.
    pub fn executor(&self, row: Row, i: u32) -> u32 {
        self.row_base(row) + self.bank * self.size + (i - 1)
    }
    /// The executor numbers of this section's current window, every row.
    fn window(&self) -> Vec<u32> {
        let rows: &[Row] = match self.kind {
            SectionKind::Strips(_) => &[Row::KeyA, Row::Fader, Row::KeyB],
            SectionKind::Keys(_) => &[Row::KeyA],
        };
        rows.iter().flat_map(|r| (1..=self.size).map(move |i| (r, i))).map(|(r, i)| self.executor(*r, i)).collect()
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub mode: PageMode,
    pub rows: Rows,
    pub ext_row: u32,
    pub mplay: bool,
    /// KB-22: a position within this distance (0..1 of the travel) of the executor's last known level picks it up.
    pub pickup_tolerance: f64,
    /// KB-22: a fader touch without a lift report for longer than this is ended locally (seconds).
    pub max_touch: f64,
    /// KB-22: a context received this long after our last position that still reports the level elsewhere is an
    /// external edit (the plugin refreshes the context at least once a second; the grace covers one refresh).
    pub external_grace: f64,
    /// KB-22: the executor whose key element the `Next (Go)` key presses (explicit profile; None = unmapped).
    pub go_executor: Option<u32>,
    /// KB-22: the executor whose key element the `Pause (||/Back)` key presses (explicit profile; None = unmapped).
    pub pause_executor: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: true, mode: PageMode::Follow, rows: Rows { key_a: 101, fader: 201, key_b: 301 }, ext_row: 401, mplay: true,
                 pickup_tolerance: 0.02, max_touch: 30.0, external_grace: 1.2, go_executor: None, pause_executor: None }
    }
}

/// KB-22: what the plugin's context last said about a bound executor, for the playback faders' pickup.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecHint {
    /// The plugin reports it as a playback target (assigned, not covered, reserved or on a missing page).
    pub target: bool,
    /// The configured fader function's level as 0..1 of the travel (the context's `lvl` / 100), when read.
    pub level: Option<f64>,
    /// The configured fader function as the console names it (`fd`), when assigned.
    pub function: Option<String>,
    /// When the context carrying it arrived (the link's clock).
    pub received_at: f64,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub bank_changes: u64,
    /// Bank keys pressed at the edge of the row (nothing changed).
    pub bank_edge: u64,
    pub downs: u64,
    pub ups: u64,
    /// Releases that went to a target the section no longer maps (a bank changed while held).
    pub frozen_releases: u64,
    /// Playback fader reports of a disabled or absent section (nothing is operated for them).
    pub ignored_faders: u64,
    /// Keys of neither a bank nor a playback button (command keys, MF keys, pressure keys).
    pub unmapped_keys: u64,
    /// KB-22: fader touches (holds on the executor's fader element) and lifts.
    pub fader_touches: u64,
    pub fader_lifts: u64,
    /// KB-22: positions sent after pickup.
    pub positions: u64,
    /// KB-22: reports that waited for pickup (level unknown, or not yet crossed).
    pub waiting_pickup: u64,
    /// KB-22: pickups (the strip reached the executor's level).
    pub pickups: u64,
    /// KB-22: pickups reset by an external edit of the level while the strip stayed touched.
    pub pickup_resets: u64,
    /// KB-22: reports dropped because the binding moved (generation, bank, rebind) while the strip stayed touched.
    pub rebound_dropped: u64,
    /// KB-22: fader touches ended locally (max_touch) or by a lost device.
    pub forced_lifts: u64,
    /// KB-22: dedicated-key presses (Go / Pause through the explicit profile).
    pub dedicated: u64,
}

/// One control event for the link, a bank change, or (KB-22) a fader bar write.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Button { dev: &'static str, control: &'static str, target: CtlTarget, kind: CtlKind },
    Bank { section: usize, bank: u32 },
    /// KB-22: a playback fader's touch, position or lift on the executor's fader element.
    Fader { dev: &'static str, control: &'static str, target: CtlTarget, kind: CtlKind },
    /// KB-22: the pickup display of a fader's RGB bar (`data` all zero clears it).
    Bar { model: crate::mtouch::Model, id: u16, data: [u8; 9] },
}

pub const PB: [&str; 10] = ["PB1", "PB2", "PB3", "PB4", "PB5", "PB6", "PB7", "PB8", "PB9", "PB10"];
pub const MPF: [&str; 12] = ["MPF1", "MPF2", "MPF3", "MPF4", "MPF5", "MPF6", "MPF7", "MPF8", "MPF9", "MPF10", "MPF11", "MPF12"];
/// KB-22: the dedicated transport keys (the same ids on the M-Touch and the M-Play).
pub const KEY_GO: u16 = 0x5513;
pub const KEY_PAUSE: u16 = 0x5512;

pub const PFA: [&str; 10] = ["PFA1", "PFA2", "PFA3", "PFA4", "PFA5", "PFA6", "PFA7", "PFA8", "PFA9", "PFA10"];
pub const PFB: [&str; 10] = ["PFB1", "PFB2", "PFB3", "PFB4", "PFB5", "PFB6", "PFB7", "PFB8", "PFB9", "PFB10"];
pub const MPA: [&str; 12] = ["MPA1", "MPA2", "MPA3", "MPA4", "MPA5", "MPA6", "MPA7", "MPA8", "MPA9", "MPA10", "MPA11", "MPA12"];
pub const MPB: [&str; 12] = ["MPB1", "MPB2", "MPB3", "MPB4", "MPB5", "MPB6", "MPB7", "MPB8", "MPB9", "MPB10", "MPB11", "MPB12"];
pub const MPE: [&str; 24] = ["MPE1", "MPE2", "MPE3", "MPE4", "MPE5", "MPE6", "MPE7", "MPE8", "MPE9", "MPE10", "MPE11", "MPE12", "MPE13", "MPE14", "MPE15", "MPE16", "MPE17", "MPE18", "MPE19", "MPE20", "MPE21", "MPE22", "MPE23", "MPE24"];

pub const MTOUCH: usize = 0;
pub const MPLAY_LEFT: usize = 1;
pub const MPLAY_EXT: usize = 2;

/// A playback button: its section, strip (1-based), row and control name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    pub section: usize,
    pub strip: u32,
    pub row: Row,
    pub control: &'static str,
}

/// The M-Touch and M-Play playback button ids (KB-16 records).
pub fn button_of(id: u16) -> Option<Button> {
    if (0x4201..=0x4292).contains(&id) && (id & 0x0f == 1 || id & 0x0f == 2) {
        let n = ((id - 0x4200) >> 4) as u32 + 1;
        if n > 10 {
            return None;
        }
        return Some(if id & 0x0f == 1 { Button { section: MTOUCH, strip: n, row: Row::KeyA, control: PFA[n as usize - 1] } } else { Button { section: MTOUCH, strip: n, row: Row::KeyB, control: PFB[n as usize - 1] } });
    }
    if id & 0x0f == 5 {
        if (0x9205..=0x92B5).contains(&id) {
            let n = ((id - 0x9205) >> 4) as u32 + 1;
            return Some(Button { section: MPLAY_LEFT, strip: n, row: Row::KeyA, control: MPA[n as usize - 1] });
        }
        if (0x92C5..=0x9375).contains(&id) {
            let n = ((id - 0x92C5) >> 4) as u32 + 1;
            return Some(Button { section: MPLAY_LEFT, strip: n, row: Row::KeyB, control: MPB[n as usize - 1] });
        }
        if (0x9805..=0x98B5).contains(&id) {
            let n = ((id - 0x9805) >> 4) as u32 + 1;
            return Some(Button { section: MPLAY_EXT, strip: n, row: Row::KeyA, control: MPE[n as usize - 1] });
        }
        if (0x98C5..=0x9975).contains(&id) {
            let n = ((id - 0x98C5) >> 4) as u32 + 13;
            return Some(Button { section: MPLAY_EXT, strip: n, row: Row::KeyA, control: MPE[n as usize - 1] });
        }
    }
    None
}

/// The bank navigation keys: (section, +1 / -1).
pub fn bank_key_of(id: u16) -> Option<(usize, i32)> {
    match id {
        0x4111 => Some((MTOUCH, 1)),
        0x4113 => Some((MTOUCH, -1)),
        0x9F12 => Some((MPLAY_LEFT, 1)),
        0x9F13 => Some((MPLAY_LEFT, -1)),
        0x9E12 => Some((MPLAY_EXT, 1)),
        0x9E13 => Some((MPLAY_EXT, -1)),
        _ => None,
    }
}

/// A playback fader report (M-Touch `LEVEL PLAYBACK n` 0x42n3, M-Play `FADER LEVEL n` 0x92n3): (section, strip).
pub fn playback_fader_of(id: u16) -> Option<(usize, u32)> {
    if id & 0x0f == 3 {
        if (0x4203..=0x4293).contains(&id) {
            return Some((MTOUCH, ((id - 0x4203) >> 4) as u32 + 1));
        }
        if (0x9203..=0x92B3).contains(&id) {
            return Some((MPLAY_LEFT, ((id - 0x9203) >> 4) as u32 + 1));
        }
    }
    None
}

/// KB-22: one playback fader's touch state.
#[derive(Debug, Clone, Default)]
struct FaderState {
    touching: bool,
    /// The executor the touch-down resolved (frozen until the lift).
    target: Option<CtlTarget>,
    generation: Option<u64>,
    rebound: bool,
    picked_up: bool,
    last_v: f64,
    since: f64,
    last_sent_at: f64,
    last_sent_v: f64,
    /// Review (PR #7): the positions sent recently (time, value), so a context's level is compared with what was in
    /// effect when the plugin read it (its refresh lags up to a second), not only with the newest position.
    sent: std::collections::VecDeque<(f64, f64)>,
    /// When the oldest position not yet seen back in a context was sent; None once a context confirmed our level.
    unconfirmed_since: Option<f64>,
    /// The context already judged (its arrival time), so each one counts once.
    judged_at: f64,
    /// What the bar last showed (None = cleared), so writes go out on change only.
    bar: Option<[u8; 9]>,
}

pub struct Banks {
    pub cfg: Config,
    pub sections: Vec<Section>,
    /// Targets of the buttons currently down, by (device, control): what the release goes to.
    held: BTreeMap<(&'static str, &'static str), CtlTarget>,
    /// KB-22: the playback faders by (section, strip).
    faders: BTreeMap<(usize, u32), FaderState>,
    /// KB-22: the M-Play pressure buttons currently down (by id), so a pressure stream is one down and one up.
    pressed: std::collections::BTreeSet<u16>,
    rebind: bool,
    pub stats: Stats,
    pub log: Vec<String>,
}

impl Banks {
    pub fn new(cfg: Config) -> Banks {
        let sections = vec![
            Section { name: "mtouch", dev: "mtouch", model: crate::mtouch::Model::MTouch, display: 0x4401, size: 10, bank: 0, enabled: cfg.enabled, present: false, kind: SectionKind::Strips(cfg.rows) },
            Section { name: "mplay-left", dev: "mplay", model: crate::mtouch::Model::MPlay, display: 0x9F01, size: 12, bank: 0, enabled: cfg.enabled && cfg.mplay, present: false, kind: SectionKind::Strips(cfg.rows) },
            Section { name: "mplay-ext", dev: "mplay", model: crate::mtouch::Model::MPlay, display: 0x9E01, size: 24, bank: 0, enabled: cfg.enabled && cfg.mplay, present: false, kind: SectionKind::Keys(cfg.ext_row) },
        ];
        Banks { cfg, sections, held: BTreeMap::new(), faders: BTreeMap::new(), pressed: Default::default(), rebind: true, stats: Stats::default(), log: Vec::new() }
    }

    fn page(&self) -> Option<u32> {
        match self.cfg.mode {
            PageMode::Follow => None,
            PageMode::Page(p) => Some(p),
        }
    }

    /// A device appeared or went away: its sections are bound only while it is attached (an absent device's
    /// window is not watched on the console). A change asks for a rebind.
    pub fn set_present(&mut self, model: crate::mtouch::Model, present: bool) {
        for s in self.sections.iter_mut().filter(|s| s.model == model) {
            if s.present != present {
                s.present = present;
                if s.enabled {
                    self.rebind = true;
                    self.log.push(format!("{}: {}", s.name, if present { "attached; its bank window is bound" } else { "absent; its bank window is unbound" }));
                }
            }
        }
    }

    /// The executor list the plugin should bind: every enabled and present section's current window, sorted,
    /// deduplicated, plus the dedicated keys' executors (KB-22). The plugin bounds the list (96); the M-Touch (30)
    /// and the M-Play (60) fit together.
    pub fn targets(&self) -> BindSpec {
        let mut ex: Vec<u32> = self.sections.iter().filter(|s| s.enabled && s.present).flat_map(|s| s.window()).collect();
        if self.cfg.enabled && self.sections.iter().any(|s| s.enabled && s.present) {
            ex.extend(self.cfg.go_executor.into_iter().chain(self.cfg.pause_executor));
        }
        ex.sort_unstable();
        ex.dedup();
        BindSpec { ex, pg: self.page() }
    }

    /// True once after a bank change (or at start): the caller re-sends the binding.
    pub fn take_rebind(&mut self) -> bool {
        std::mem::take(&mut self.rebind)
    }

    pub fn target(&self, b: Button) -> CtlTarget {
        let s = &self.sections[b.section];
        CtlTarget::Executor { ex: s.executor(b.row, b.strip), el: "key", pg: self.page() }
    }

    /// Moves a section's bank by `delta` within its row; nothing changes at the edge. Held buttons keep their targets.
    pub fn bank_step(&mut self, section: usize, delta: i32) -> Option<Out> {
        let s = &mut self.sections[section];
        if !s.enabled {
            return None;
        }
        let max = s.max_bank() as i64;
        let next = s.bank as i64 + delta as i64;
        if next < 0 || next > max {
            self.stats.bank_edge += 1;
            self.log.push(format!("{}: bank {} is the {} of the row (banks 1-{}); unchanged", s.name, s.bank + 1, if next < 0 { "first" } else { "last" }, max + 1));
            return None;
        }
        s.bank = next as u32;
        self.stats.bank_changes += 1;
        self.rebind = true;
        let first = s.executor(Row::KeyA, 1);
        let last = s.executor(Row::KeyA, s.size);
        let held = self.held.iter().filter(|((dev, _), _)| *dev == s.dev).count();
        self.log.push(format!("{}: bank {} (executors {}-{} per row){}", s.name, s.bank + 1, first, last, if held > 0 { format!("; {held} held button(s) keep their targets until released") } else { String::new() }));
        Some(Out::Bank { section, bank: s.bank })
    }

    /// A key of the M-Touch or M-Play went down or up. Bank keys move the window on their down; playback buttons
    /// go out as `btn` events with a frozen target; everything else is counted. The device is inferred from the id
    /// (the dedicated keys share ids: use `key_on` for them).
    pub fn key(&mut self, id: u16, down: bool) -> Option<Out> {
        self.key_on(if id >= 0x9000 { "mplay" } else { "mtouch" }, id, down)
    }

    /// KB-22: a key of a named device. The dedicated `Next (Go)` and `Pause` keys press the profile's executors.
    pub fn key_on(&mut self, dev: &'static str, id: u16, down: bool) -> Option<Out> {
        if let Some((section, delta)) = bank_key_of(id) {
            if down {
                return self.bank_step(section, delta);
            }
            return None;
        }
        let dedicated = match id {
            KEY_GO => self.cfg.go_executor.map(|ex| (ex, "Go")),
            KEY_PAUSE => self.cfg.pause_executor.map(|ex| (ex, "Pause")),
            _ => None,
        };
        if let Some((ex, control)) = dedicated {
            if !self.cfg.enabled {
                self.stats.unmapped_keys += 1;
                return None;
            }
            let dev = if dev == "mplay" { "mplay" } else { "mtouch" };
            let current = CtlTarget::Executor { ex, el: "key", pg: self.page() };
            if down {
                self.held.insert((dev, control), current);
                self.stats.downs += 1;
                self.stats.dedicated += 1;
                return Some(Out::Button { dev, control, target: current, kind: CtlKind::Btn { down: true } });
            }
            self.stats.ups += 1;
            let target = self.held.remove(&(dev, control)).unwrap_or(current);
            return Some(Out::Button { dev, control, target, kind: CtlKind::Btn { down: false } });
        }
        let Some(b) = button_of(id) else {
            self.stats.unmapped_keys += 1;
            return None;
        };
        let dev = self.sections[b.section].dev;
        if !self.sections[b.section].enabled {
            self.stats.unmapped_keys += 1;
            return None;
        }
        if down {
            let target = self.target(b);
            self.held.insert((dev, b.control), target);
            self.stats.downs += 1;
            Some(Out::Button { dev, control: b.control, target, kind: CtlKind::Btn { down: true } })
        } else {
            self.stats.ups += 1;
            let current = self.target(b);
            let target = match self.held.remove(&(dev, b.control)) {
                Some(t) => {
                    if t != current {
                        self.stats.frozen_releases += 1;
                        self.log.push(format!("{}/{}: released {} (pressed before the bank moved; now {})", dev, b.control, describe(&t), describe(&current)));
                    }
                    t
                }
                None => current,
            };
            Some(Out::Button { dev, control: b.control, target, kind: CtlKind::Btn { down: false } })
        }
    }

    /// KB-22: a pressure-key report of the M-Play's playback buttons (kind V: pressure 0..255, `touch` while the
    /// finger is on it). The first report with `touch` is the down, the release report the up; the pressure value
    /// is never an input. Other pressure keys (the M-Touch's PFD keys, MF keys) are counted unmapped once per press.
    pub fn pressure(&mut self, dev: &'static str, id: u16, _value: u8, touch: bool) -> Option<Out> {
        let was = self.pressed.contains(&id);
        if touch && !was {
            self.pressed.insert(id);
            return self.key_on(dev, id, true);
        }
        if !touch && was {
            self.pressed.remove(&id);
            return self.key_on(dev, id, false);
        }
        None
    }

    /// KB-22: the executor a playback fader means right now (the fader row of its section at the current bank).
    pub fn fader_executor(&self, section: usize, strip: u32) -> (u32, Option<u32>) {
        (self.sections[section].executor(Row::Fader, strip), self.page())
    }

    fn fader_control(section: usize, strip: u32) -> &'static str {
        if section == MTOUCH { PB[(strip.clamp(1, 10) - 1) as usize] } else { MPF[(strip.clamp(1, 12) - 1) as usize] }
    }

    /// KB-22: the RGB bar id of a playback fader (its own control id: M-Touch 0x42n3, M-Play 0x92n3).
    pub fn fader_bar_id(section: usize, strip: u32) -> u16 {
        if section == MTOUCH { 0x4203 + 0x10 * (strip.clamp(1, 10) as u16 - 1) } else { 0x9203 + 0x10 * (strip.clamp(1, 12) as u16 - 1) }
    }

    /// KB-22: the pickup display: the LEDs between the strip's position and the executor's level, yellow, blinking.
    pub fn pickup_bar(v: f64, level: f64) -> [u8; 9] {
        let pos = (v.clamp(0.0, 1.0) * 9.0).round() as u16;
        let tgt = (level.clamp(0.0, 1.0) * 9.0).round() as u16;
        let (lo, hi) = (pos.min(tgt), pos.max(tgt));
        let mask: u16 = ((1u32 << (hi + 1)) - (1u32 << lo)) as u16;
        crate::mtouch::fader_bar(mask, mask, 0, (true, true, false))
    }

    fn bar_out(&mut self, section: usize, strip: u32, data: Option<[u8; 9]>) -> Option<Out> {
        let st = self.faders.entry((section, strip)).or_default();
        if st.bar == data {
            return None;
        }
        st.bar = data;
        Some(Out::Bar { model: self.sections[section].model, id: Self::fader_bar_id(section, strip), data: data.unwrap_or([0u8; 9]) })
    }

    /// KB-22: one report of a playback fader (`value` 0..255, `touch` while the finger is on it; the lift report
    /// has `touch` false). `cg` is the binding generation (None: unbound), `hint` the executor's last known level.
    pub fn fader(&mut self, section: usize, strip: u32, value: u8, touch: bool, cg: Option<u64>, hint: Option<&ExecHint>, now: f64) -> Vec<Out> {
        let mut outs = Vec::new();
        let s = &self.sections[section];
        if !self.cfg.enabled || !s.enabled || !s.present || !matches!(s.kind, SectionKind::Strips(_)) {
            self.stats.ignored_faders += 1;
            return outs;
        }
        let dev = s.dev;
        let control = Self::fader_control(section, strip);
        let v = value as f64 / 255.0;
        let touching = self.faders.get(&(section, strip)).map(|f| f.touching).unwrap_or(false);
        if !touch {
            // The lift report (or a resting / startup report while nothing is touched: nothing is seeded from it).
            if touching {
                let st = self.faders.get_mut(&(section, strip)).expect("touching implies a state");
                let target = st.target.take().expect("a touch has a target");
                st.touching = false;
                st.picked_up = false;
                self.stats.fader_lifts += 1;
                outs.push(Out::Fader { dev, control, target, kind: CtlKind::Touch { down: false } });
                outs.extend(self.bar_out(section, strip, None));
            }
            return outs;
        }
        if !touching {
            let (ex, pg) = self.fader_executor(section, strip);
            let target = CtlTarget::Executor { ex, el: "fader", pg };
            let st = self.faders.entry((section, strip)).or_default();
            st.touching = true;
            st.target = Some(target);
            st.generation = cg;
            st.rebound = cg.is_none();
            st.picked_up = false;
            st.last_v = v;
            st.since = now;
            st.sent.clear();
            st.unconfirmed_since = None;
            st.judged_at = f64::NEG_INFINITY;
            self.stats.fader_touches += 1;
            outs.push(Out::Fader { dev, control, target, kind: CtlKind::Touch { down: true } });
            return outs;
        }
        let tol = self.cfg.pickup_tolerance;
        let grace = self.cfg.external_grace;
        let st = self.faders.get_mut(&(section, strip)).expect("touching implies a state");
        let target = st.target.expect("a touch has a target");
        if st.generation != cg {
            st.rebound = true;
        }
        if st.rebound {
            st.last_v = v;
            self.stats.rebound_dropped += 1;
            outs.extend(self.bar_out(section, strip, None));
            return outs;
        }
        let level = hint.filter(|h| h.target).and_then(|h| h.level);
        // An external edit (review of PR #7): each context is judged once against the positions that were in effect
        // when the plugin could have read it (the last `grace` before its arrival, plus the one before that window).
        // A match confirms our level; no match for longer than `grace` since the first unconfirmed position means
        // the console is not following the strip: someone else moved it. Continuous reports never restart the clock.
        if st.picked_up {
            if let (Some(h), Some(l)) = (hint, level) {
                if h.received_at > st.judged_at {
                    st.judged_at = h.received_at;
                    let window_start = h.received_at - grace;
                    let before = st.sent.iter().rev().find(|(t, _)| *t < window_start).map(|(_, v)| *v);
                    let matches = st.sent.iter().filter(|(t, _)| *t >= window_start && *t <= h.received_at).map(|(_, v)| *v).chain(before).any(|v| (v - l).abs() <= 2.0 * tol);
                    if matches {
                        st.unconfirmed_since = None;
                    } else if st.unconfirmed_since.map(|t| h.received_at > t + grace).unwrap_or(false) {
                        st.picked_up = false;
                        st.unconfirmed_since = None;
                        self.stats.pickup_resets += 1;
                        self.log.push(format!("{dev}/{control}: the executor's level moved to {:.0}% while the strip was at {:.0}% (edited elsewhere); pick it up again", l * 100.0, st.last_sent_v * 100.0));
                    }
                }
            }
        }
        if !st.picked_up {
            if let Some(l) = level {
                let crossed = (st.last_v - l) * (v - l) <= 0.0;
                if crossed || (v - l).abs() <= tol {
                    st.picked_up = true;
                    self.stats.pickups += 1;
                }
            }
        }
        st.last_v = v;
        if st.picked_up {
            st.last_sent_at = now;
            st.last_sent_v = v;
            st.sent.push_back((now, v));
            while st.sent.len() > 64 || st.sent.front().map(|(t, _)| now - *t > 4.0 * grace).unwrap_or(false) {
                st.sent.pop_front();
            }
            if st.unconfirmed_since.is_none() {
                st.unconfirmed_since = Some(now);
            }
            self.stats.positions += 1;
            outs.push(Out::Fader { dev, control, target, kind: CtlKind::Abs { v, takeover: false } });
            outs.extend(self.bar_out(section, strip, None));
        } else {
            self.stats.waiting_pickup += 1;
            let bar = level.map(|l| Self::pickup_bar(v, l));
            outs.extend(self.bar_out(section, strip, bar));
        }
        outs
    }

    /// KB-22: the plugin's binding was replaced (a bank change applied, or the plugin rebound): every touched fader
    /// is rebound (its positions dropped until the lift; the next touch picks up again), as the parameter strips are.
    pub fn rebind_faders(&mut self) -> Vec<Out> {
        let mut outs = Vec::new();
        let keys: Vec<(usize, u32)> = self.faders.iter().filter(|(_, f)| f.touching && !f.rebound).map(|(k, _)| *k).collect();
        for k in keys {
            let st = self.faders.get_mut(&k).expect("listed");
            st.rebound = true;
            st.picked_up = false;
            self.log.push(format!("{}/{}: the binding was replaced while touched; lift and touch again", self.sections[k.0].dev, Self::fader_control(k.0, k.1)));
            outs.extend(self.bar_out(k.0, k.1, None));
        }
        outs
    }

    /// KB-22: housekeeping: a moved generation rebinds touched faders; a touch past `max_touch` is ended locally.
    pub fn tick(&mut self, cg: Option<u64>, now: f64) -> Vec<Out> {
        let mut outs = Vec::new();
        let keys: Vec<(usize, u32)> = self.faders.iter().filter(|(_, f)| f.touching).map(|(k, _)| *k).collect();
        for k in keys {
            let dev = self.sections[k.0].dev;
            let control = Self::fader_control(k.0, k.1);
            let max_touch = self.cfg.max_touch;
            let st = self.faders.get_mut(&k).expect("listed");
            if st.generation != cg && !st.rebound {
                st.rebound = true;
                st.picked_up = false;
                self.log.push(format!("{dev}/{control}: the binding changed while touched; lift and touch again"));
                outs.extend(self.bar_out(k.0, k.1, None));
                continue;
            }
            if now - st.since > max_touch {
                let target = st.target.take().expect("a touch has a target");
                st.touching = false;
                st.picked_up = false;
                self.stats.forced_lifts += 1;
                self.log.push(format!("{dev}/{control}: touched for more than {max_touch:.0} s without a lift report; ended locally"));
                outs.push(Out::Fader { dev, control, target, kind: CtlKind::Touch { down: false } });
                outs.extend(self.bar_out(k.0, k.1, None));
            }
        }
        outs
    }

    /// The device went away: every button it held is released against its frozen target and (KB-22) every fader
    /// it had touched is lifted; their bars are cleared.
    pub fn disconnect(&mut self, dev: &'static str) -> Vec<Out> {
        let keys: Vec<(&'static str, &'static str)> = self.held.keys().filter(|(d, _)| *d == dev).copied().collect();
        let mut outs = Vec::new();
        for k in keys {
            if let Some(t) = self.held.remove(&k) {
                self.log.push(format!("{}/{}: {} lost while held; released {}", k.0, k.1, dev, describe(&t)));
                outs.push(Out::Button { dev: k.0, control: k.1, target: t, kind: CtlKind::Btn { down: false } });
            }
        }
        let faders: Vec<(usize, u32)> = self.faders.iter().filter(|(k, f)| f.touching && self.sections[k.0].dev == dev).map(|(k, _)| *k).collect();
        for k in faders {
            let control = Self::fader_control(k.0, k.1);
            let st = self.faders.get_mut(&k).expect("listed");
            let target = st.target.take().expect("a touch has a target");
            *st = FaderState::default();
            self.stats.forced_lifts += 1;
            self.log.push(format!("{dev}/{control}: {dev} lost while touched; lifted {}", describe(&target)));
            outs.push(Out::Fader { dev, control, target, kind: CtlKind::Touch { down: false } });
            outs.extend(self.bar_out(k.0, k.1, None));
        }
        if dev == "mplay" {
            self.pressed.clear();
        }
        outs
    }

    /// KB-22: playback faders currently touched.
    pub fn touched_count(&self) -> usize {
        self.faders.values().filter(|f| f.touching).count()
    }

    #[cfg(test)]
    pub fn fader_touching(&self, section: usize, strip: u32) -> bool {
        self.faders.get(&(section, strip)).map(|f| f.touching).unwrap_or(false)
    }

    pub fn held_count(&self) -> usize {
        self.held.len()
    }

    /// KB-23: every playback light the present, enabled sections map right now, with the executor it means and
    /// whether its button is down / its fader is touched and waiting for pickup. The M-Touch strip: PFA (blue
    /// key, row A), the RGB bar and the red/green PFD (both the fader row's executor), PFB (blue key, row B); the
    /// M-Play strip: top button (red/green, row A), the RGB bar (fader row), bottom button (red/green, row B); the
    /// M-Play right block: red/green buttons (its row). The lights of a section whose device is absent are not
    /// listed (nothing can be written to it).
    pub fn lights_controls(&self) -> Vec<crate::lights::PlaybackControl> {
        use crate::lights::{Light, PlaybackControl};
        let mut out = Vec::new();
        if !self.cfg.enabled {
            return out;
        }
        let pg = self.page();
        for (si, s) in self.sections.iter().enumerate() {
            if !s.enabled || !s.present {
                continue;
            }
            let held = |control: &'static str| self.held.contains_key(&(s.dev, control));
            match s.kind {
                SectionKind::Strips(_) => {
                    for i in 1..=s.size {
                        let n = i as usize - 1;
                        let (ka, kb, bar, pfd, light_ab) = if si == MTOUCH {
                            (0x4201 + 0x10 * (i as u16 - 1), 0x4202 + 0x10 * (i as u16 - 1), 0x4203 + 0x10 * (i as u16 - 1), Some(0x4205 + 0x10 * (i as u16 - 1)), Light::KeyBlue)
                        } else {
                            (0x9205 + 0x10 * (i as u16 - 1), 0x92C5 + 0x10 * (i as u16 - 1), 0x9203 + 0x10 * (i as u16 - 1), None, Light::KeyRedGreen)
                        };
                        let (ca, cb, cf): (&'static str, &'static str, &'static str) = if si == MTOUCH { (PFA[n], PFB[n], PB[n]) } else { (MPA[n], MPB[n], MPF[n]) };
                        let fs = self.faders.get(&(si, i));
                        let touching = fs.map(|f| f.touching).unwrap_or(false);
                        let pending = fs.map(|f| f.touching && !f.picked_up && !f.rebound).unwrap_or(false);
                        out.push(PlaybackControl { model: s.model, id: ka, light: light_ab, control: ca, ex: s.executor(Row::KeyA, i), pg, held: held(ca), pickup_pending: false });
                        out.push(PlaybackControl { model: s.model, id: bar, light: Light::BarRgb, control: cf, ex: s.executor(Row::Fader, i), pg, held: touching, pickup_pending: pending });
                        if let Some(id) = pfd {
                            out.push(PlaybackControl { model: s.model, id, light: Light::KeyRedGreen, control: cf, ex: s.executor(Row::Fader, i), pg, held: false, pickup_pending: false });
                        }
                        out.push(PlaybackControl { model: s.model, id: kb, light: light_ab, control: cb, ex: s.executor(Row::KeyB, i), pg, held: held(cb), pickup_pending: false });
                    }
                }
                SectionKind::Keys(_) => {
                    for i in 1..=s.size {
                        let n = i as usize - 1;
                        let id = if i <= 12 { 0x9805 + 0x10 * (i as u16 - 1) } else { 0x98C5 + 0x10 * (i as u16 - 13) };
                        out.push(PlaybackControl { model: s.model, id, light: Light::KeyRedGreen, control: MPE[n], ex: s.executor(Row::KeyA, i), pg, held: held(MPE[n]), pickup_pending: false });
                    }
                }
            }
        }
        out
    }

    /// The page display value of a section: page x 10 + bank while both fit, else the bank, None while the page is unknown.
    pub fn display_value(&self, section: usize, console_page: Option<u32>) -> Option<u16> {
        let s = &self.sections[section];
        if !s.enabled || !s.present {
            return None;
        }
        let page = match self.cfg.mode {
            PageMode::Follow => console_page?,
            PageMode::Page(p) => p,
        };
        let bank = s.bank + 1;
        if page <= 99 && bank <= 9 {
            Some((page * 10 + bank) as u16)
        } else {
            Some(bank.min(999) as u16)
        }
    }

    pub fn summary(&self, console_page: Option<u32>) -> String {
        let mode = match self.cfg.mode {
            PageMode::Follow => format!("follow page={}", console_page.map(|p| p.to_string()).unwrap_or_else(|| "?".into())),
            PageMode::Page(p) => format!("independent page={p}"),
        };
        let secs: Vec<String> = self.sections.iter().filter(|s| s.enabled).map(|s| if s.present { format!("{}=bank{}({}-{})", s.name, s.bank + 1, s.executor(Row::KeyA, 1), s.executor(Row::KeyA, s.size)) } else { format!("{}=absent", s.name) }).collect();
        let st = &self.stats;
        format!("banks: {} {} held={} changes={} edge={} downs={} ups={} frozen={} ignored_faders={} unmapped={} faders: touched={} touches={} lifts={} positions={} pickups={} waiting={} resets={} rebound={} forced={} dedicated={}",
                if self.cfg.enabled { mode } else { "off".into() }, secs.join(" "), self.held.len(), st.bank_changes, st.bank_edge, st.downs, st.ups, st.frozen_releases, st.ignored_faders, st.unmapped_keys,
                self.touched_count(), st.fader_touches, st.fader_lifts, st.positions, st.pickups, st.waiting_pickup, st.pickup_resets, st.rebound_dropped, st.forced_lifts, st.dedicated)
    }
}

fn describe(t: &CtlTarget) -> String {
    match t {
        CtlTarget::Executor { ex, el, pg } => format!("executor {ex} {el}{}", pg.map(|p| format!(" of page {p}")).unwrap_or_default()),
        CtlTarget::Slot { slot } => format!("slot {slot}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(n: u32) -> CtlTarget {
        CtlTarget::Executor { ex: n, el: "key", pg: None }
    }

    #[test]
    fn parses_modes_and_rows() {
        assert_eq!("follow".parse::<PageMode>().unwrap(), PageMode::Follow);
        assert_eq!("3".parse::<PageMode>().unwrap(), PageMode::Page(3));
        assert!("0".parse::<PageMode>().is_err());
        assert!("two".parse::<PageMode>().is_err());
        assert_eq!("101,201,301".parse::<Rows>().unwrap(), Rows { key_a: 101, fader: 201, key_b: 301 });
        assert!("101,201".parse::<Rows>().is_err());
        assert!("100,200,300".parse::<Rows>().is_err(), "a row base ends in 01");
    }

    #[test]
    fn ids_map_to_buttons_banks_and_faders() {
        assert_eq!(button_of(0x4201), Some(Button { section: MTOUCH, strip: 1, row: Row::KeyA, control: "PFA1" }));
        assert_eq!(button_of(0x4292), Some(Button { section: MTOUCH, strip: 10, row: Row::KeyB, control: "PFB10" }));
        assert_eq!(button_of(0x4205), None, "the pressure key is not a button");
        assert_eq!(button_of(0x9205).unwrap().control, "MPA1");
        assert_eq!(button_of(0x92B5).unwrap().control, "MPA12");
        assert_eq!(button_of(0x92C5).unwrap().control, "MPB1");
        assert_eq!(button_of(0x9375).unwrap().control, "MPB12");
        assert_eq!(button_of(0x9805).unwrap().control, "MPE1");
        assert_eq!(button_of(0x98B5).unwrap().control, "MPE12");
        assert_eq!(button_of(0x98C5).unwrap().control, "MPE13");
        assert_eq!(button_of(0x9975).unwrap().control, "MPE24");
        assert_eq!(button_of(0x5513), None, "Go is a command key, unmapped");
        assert_eq!(bank_key_of(0x4111), Some((MTOUCH, 1)));
        assert_eq!(bank_key_of(0x9F13), Some((MPLAY_LEFT, -1)));
        assert_eq!(bank_key_of(0x9E12), Some((MPLAY_EXT, 1)));
        assert_eq!(playback_fader_of(0x4203), Some((MTOUCH, 1)));
        assert_eq!(playback_fader_of(0x4293), Some((MTOUCH, 10)));
        assert_eq!(playback_fader_of(0x92B3), Some((MPLAY_LEFT, 12)));
        assert_eq!(playback_fader_of(0x6112), None, "a parameter strip is not a playback fader");
    }

    fn present(mut b: Banks) -> Banks {
        b.set_present(crate::mtouch::Model::MTouch, true);
        b.set_present(crate::mtouch::Model::MPlay, true);
        b.take_rebind();
        b
    }

    #[test]
    fn only_present_devices_are_bound() {
        let mut b = Banks::new(Config::default());
        assert!(b.take_rebind());
        assert_eq!(b.targets().ex.len(), 0, "nothing attached: nothing bound");
        b.set_present(crate::mtouch::Model::MTouch, true);
        assert!(b.take_rebind());
        assert_eq!(b.targets().ex.len(), 30);
        assert_eq!(b.display_value(MPLAY_LEFT, Some(1)), None, "an absent device has no display");
        b.set_present(crate::mtouch::Model::MPlay, true);
        assert_eq!(b.targets().ex.len(), 60);
        b.key(0x4111, true);
        assert_eq!(b.targets().ex.len(), 84, "both attached, the M-Touch on bank 2: within the plugin's bound of 96");
        b.set_present(crate::mtouch::Model::MTouch, false);
        assert!(b.take_rebind());
        assert_eq!(b.targets().ex.len(), 60);
        assert!(b.summary(Some(1)).contains("mtouch=absent"));
    }

    #[test]
    fn the_default_profile_maps_strip_i_to_rows_101_201_301_and_banks_by_ten() {
        let mut b = present(Banks::new(Config::default()));
        assert_eq!(b.target(button_of(0x4201).unwrap()), ex(101));
        assert_eq!(b.target(button_of(0x4292).unwrap()), ex(310));
        let t = b.targets();
        assert_eq!(t.pg, None, "follow mode binds without a page");
        // M-Touch 101-110/201-210/301-310 lie inside the M-Play left block's 101-112/201-212/301-312: 36 + the right block's 24.
        assert_eq!(t.ex.len(), 36 + 24);
        assert!(t.ex.windows(2).all(|w| w[0] < w[1]), "sorted, deduplicated");
        assert!(t.ex.contains(&101) && t.ex.contains(&210) && t.ex.contains(&310) && t.ex.contains(&112) && t.ex.contains(&424));
        assert!(!b.take_rebind(), "nothing changed since the presence was set");
        assert_eq!(b.key(0x4111, true), Some(Out::Bank { section: MTOUCH, bank: 1 }));
        assert_eq!(b.target(button_of(0x4201).unwrap()), ex(111));
        assert_eq!(b.target(button_of(0x4292).unwrap()), ex(320));
        assert!(b.take_rebind());
        assert_eq!(b.key(0x4111, false), None, "the key's release does nothing");
        for _ in 0..7 {
            b.key(0x4111, true);
        }
        assert_eq!(b.sections[MTOUCH].bank, 8, "bank 9 is the last of the row (181-190 / 281-290 / 381-390)");
        assert_eq!(b.target(button_of(0x4292).unwrap()), ex(390));
        assert_eq!(b.key(0x4111, true), None, "past the edge nothing changes");
        assert_eq!(b.stats.bank_edge, 1);
        for _ in 0..9 {
            b.key(0x4113, true);
        }
        assert_eq!(b.sections[MTOUCH].bank, 0);
        assert_eq!(b.stats.bank_edge, 2);
        // M-Play: 12 per bank on the left (banks 1-7), 24 on the right (banks 1-3).
        assert_eq!(b.sections[MPLAY_LEFT].max_bank(), 6);
        assert_eq!(b.sections[MPLAY_EXT].max_bank(), 2);
        assert_eq!(b.target(button_of(0x9975).unwrap()), ex(424));
        b.key(0x9E12, true);
        assert_eq!(b.target(button_of(0x9805).unwrap()), ex(425));
        assert_eq!(b.target(button_of(0x92C5).unwrap()), ex(301), "the left section did not move");
    }

    #[test]
    fn independent_page_binds_with_the_page_and_the_target_carries_it() {
        let b = present(Banks::new(Config { mode: PageMode::Page(4), ..Config::default() }));
        assert_eq!(b.targets().pg, Some(4));
        assert_eq!(b.target(button_of(0x4201).unwrap()), CtlTarget::Executor { ex: 101, el: "key", pg: Some(4) });
        assert_eq!(b.display_value(MTOUCH, None), Some(41), "page 4 bank 1 even without a context");
        assert_eq!(b.display_value(MTOUCH, Some(9)), Some(41), "the console's page is not the bound one");
    }

    #[test]
    fn a_held_button_keeps_its_target_across_a_bank_change() {
        let mut b = present(Banks::new(Config::default()));
        let down = b.key(0x4201, true).unwrap();
        assert_eq!(down, Out::Button { dev: "mtouch", control: "PFA1", target: ex(101), kind: CtlKind::Btn { down: true } });
        assert_eq!(b.held_count(), 1);
        b.key(0x4111, true);
        assert!(b.log.last().unwrap().contains("1 held button(s) keep their targets"));
        let up = b.key(0x4201, false).unwrap();
        assert_eq!(up, Out::Button { dev: "mtouch", control: "PFA1", target: ex(101), kind: CtlKind::Btn { down: false } }, "the release goes to 101, not to 111");
        assert_eq!(b.stats.frozen_releases, 1);
        assert_eq!(b.held_count(), 0);
        let again = b.key(0x4201, true).unwrap();
        assert_eq!(again, Out::Button { dev: "mtouch", control: "PFA1", target: ex(111), kind: CtlKind::Btn { down: true } }, "a fresh press reaches the new mapping");
        // A release without a recorded press (the press was lost) goes to the current mapping.
        let stray = b.key(0x4202, false).unwrap();
        assert_eq!(stray, Out::Button { dev: "mtouch", control: "PFB1", target: ex(311), kind: CtlKind::Btn { down: false } });
    }

    #[test]
    fn a_lost_device_releases_what_it_held() {
        let mut b = present(Banks::new(Config::default()));
        b.key(0x4201, true);
        b.key(0x9205, true);
        let outs = b.disconnect("mtouch");
        assert_eq!(outs, vec![Out::Button { dev: "mtouch", control: "PFA1", target: ex(101), kind: CtlKind::Btn { down: false } }]);
        assert_eq!(b.held_count(), 1, "the M-Play's button is still down");
    }

    #[test]
    fn displays_and_summary() {
        let mut b = present(Banks::new(Config::default()));
        assert_eq!(b.display_value(MTOUCH, None), None, "page unknown: blank");
        assert_eq!(b.display_value(MTOUCH, Some(1)), Some(11));
        b.key(0x4111, true);
        assert_eq!(b.display_value(MTOUCH, Some(3)), Some(32));
        assert_eq!(b.display_value(MTOUCH, Some(120)), Some(2), "a page beyond two digits: the bank alone");
        assert_eq!(b.display_value(MPLAY_EXT, Some(1)), Some(11));
        let s = b.summary(Some(3));
        assert!(s.starts_with("banks: follow page=3 mtouch=bank2(111-120) mplay-left=bank1(101-112) mplay-ext=bank1(401-424) held=0 changes=1"), "{s}");
        let off = Banks::new(Config { enabled: false, ..Config::default() });
        assert!(off.summary(None).starts_with("banks: off"));
        assert_eq!(off.targets().ex.len(), 0);
        let no_mplay = present(Banks::new(Config { mplay: false, ..Config::default() }));
        assert_eq!(no_mplay.targets().ex.len(), 30);
        assert_eq!(no_mplay.display_value(MPLAY_LEFT, Some(1)), None);
    }

    #[test]
    fn unmapped_keys_and_faders_of_absent_sections_are_counted_not_dispatched() {
        let mut b = present(Banks::new(Config::default()));
        assert_eq!(b.key(0x5513, true), None, "Go without a profile is unmapped");
        assert_eq!(b.key(0x4205, true), None);
        assert_eq!(b.stats.unmapped_keys, 2);
        let mut off = Banks::new(Config { enabled: false, ..Config::default() });
        assert!(off.fader(MTOUCH, 1, 100, true, Some(1), None, 0.0).is_empty());
        assert_eq!(off.stats.ignored_faders, 1);
        let mut absent = Banks::new(Config::default());
        assert!(absent.fader(MPLAY_LEFT, 1, 100, true, Some(1), None, 0.0).is_empty(), "an absent device's fader is ignored");
        assert_eq!(absent.stats.ignored_faders, 1);
    }

    // ---- KB-22: playback faders ----
    fn hint(level: f64, at: f64) -> ExecHint {
        ExecHint { target: true, level: Some(level), function: Some("Master".into()), received_at: at }
    }
    fn fader_target(n: u32) -> CtlTarget {
        CtlTarget::Executor { ex: n, el: "fader", pg: None }
    }
    fn kinds(outs: &[Out]) -> Vec<CtlKind> {
        outs.iter().filter_map(|o| if let Out::Fader { kind, .. } = o { Some(*kind) } else { None }).collect()
    }
    fn bars(outs: &[Out]) -> Vec<bool> {
        outs.iter().filter_map(|o| if let Out::Bar { data, .. } = o { Some(*data != [0u8; 9]) } else { None }).collect()
    }

    #[test]
    fn a_fader_touch_is_a_hold_on_the_fader_row_executor_and_positions_wait_for_pickup() {
        let mut b = present(Banks::new(Config::default()));
        let h = hint(0.5, 0.0);
        let t = b.fader(MTOUCH, 1, 20, true, Some(1), Some(&h), 1.0);
        assert_eq!(t, vec![Out::Fader { dev: "mtouch", control: "PB1", target: fader_target(201), kind: CtlKind::Touch { down: true } }], "the touch-down is a hold on executor 201's fader; the position moves nothing");
        assert!(b.fader_touching(MTOUCH, 1));
        let w = b.fader(MTOUCH, 1, 40, true, Some(1), Some(&h), 1.1);
        assert!(kinds(&w).is_empty(), "below the level: waiting for pickup, nothing sent");
        assert_eq!(bars(&w), vec![true], "the bar shows the span to travel");
        assert_eq!(b.stats.waiting_pickup, 1);
        let p = b.fader(MTOUCH, 1, 140, true, Some(1), Some(&h), 1.2);
        assert_eq!(kinds(&p), vec![CtlKind::Abs { v: 140.0 / 255.0, takeover: false }], "crossing the level picks it up: the position goes out");
        assert_eq!(bars(&p), vec![false], "the bar is cleared on pickup");
        assert_eq!(b.stats.pickups, 1);
        let p2 = b.fader(MTOUCH, 1, 10, true, Some(1), Some(&h), 1.3);
        assert_eq!(kinds(&p2), vec![CtlKind::Abs { v: 10.0 / 255.0, takeover: false }], "once picked up every report is a position");
        let l = b.fader(MTOUCH, 1, 10, false, Some(1), Some(&h), 1.4);
        assert_eq!(kinds(&l), vec![CtlKind::Touch { down: false }], "the lift ends the hold on the frozen target");
        assert!(!b.fader_touching(MTOUCH, 1));
        assert_eq!(b.stats.positions, 2);
        // Within the tolerance counts as pickup too; a resting report while nothing is touched seeds nothing.
        let t2 = b.fader(MTOUCH, 2, 130, true, Some(1), Some(&h), 2.0);
        assert_eq!(kinds(&t2), vec![CtlKind::Touch { down: true }]);
        let p3 = b.fader(MTOUCH, 2, 129, true, Some(1), Some(&h), 2.1);
        assert_eq!(kinds(&p3).len(), 1, "129/255 = 0.506 is within 0.02 of 0.5");
        b.fader(MTOUCH, 2, 129, false, Some(1), Some(&h), 2.2);
        assert!(b.fader(MTOUCH, 3, 200, false, Some(1), Some(&h), 3.0).is_empty(), "a startup/resting report (touch 0, not touching) is nothing");
        assert_eq!(b.stats.fader_touches, 2);
        assert_eq!(b.stats.fader_lifts, 2);
    }

    #[test]
    fn an_unknown_level_never_picks_up_and_the_fader_waits() {
        let mut b = present(Banks::new(Config::default()));
        b.fader(MTOUCH, 1, 0, true, Some(1), None, 1.0);
        for v in [50u8, 120, 255] {
            let o = b.fader(MTOUCH, 1, v, true, Some(1), None, 1.1);
            assert!(kinds(&o).is_empty() && bars(&o).is_empty(), "no context: nothing sent, no bar (no level to show)");
        }
        let not_target = ExecHint { target: false, level: Some(0.5), function: None, received_at: 0.0 };
        assert!(kinds(&b.fader(MTOUCH, 1, 0, true, Some(1), Some(&not_target), 1.2)).is_empty(), "an executor that is not a playback target is never picked up");
        assert_eq!(b.stats.waiting_pickup, 4);
        assert_eq!(b.stats.positions, 0);
    }

    #[test]
    fn a_bank_change_or_generation_move_rebinds_a_touched_fader_and_the_next_touch_picks_up_again() {
        let mut b = present(Banks::new(Config::default()));
        let h = hint(0.2, 0.0);
        b.fader(MTOUCH, 1, 0, true, Some(1), Some(&h), 1.0);
        assert_eq!(kinds(&b.fader(MTOUCH, 1, 60, true, Some(1), Some(&h), 1.1)).len(), 1, "picked up");
        b.key(0x4111, true);  // bank 2: the fader now means 211, but the touch stays frozen to 201
        let r = b.rebind_faders();
        assert!(bars(&r).is_empty(), "nothing to clear: the bar was already clear");
        assert!(kinds(&b.fader(MTOUCH, 1, 80, true, Some(2), Some(&h), 1.2)).is_empty(), "rebound: positions are dropped until the lift");
        assert_eq!(b.stats.rebound_dropped, 1);
        let l = b.fader(MTOUCH, 1, 80, false, Some(2), Some(&h), 1.3);
        assert_eq!(l, vec![Out::Fader { dev: "mtouch", control: "PB1", target: fader_target(201), kind: CtlKind::Touch { down: false } }], "the lift goes to the frozen 201, not to 211");
        let t = b.fader(MTOUCH, 1, 80, true, Some(2), Some(&h), 1.4);
        assert_eq!(t, vec![Out::Fader { dev: "mtouch", control: "PB1", target: fader_target(211), kind: CtlKind::Touch { down: true } }], "a fresh touch means the new mapping");
        assert!(kinds(&b.fader(MTOUCH, 1, 90, true, Some(2), Some(&h), 1.5)).is_empty(), "and waits for pickup again");
        // A generation move seen by tick rebinds too.
        let o = b.tick(Some(3), 1.6);
        assert_eq!(bars(&o), vec![false], "the rebind clears the pickup bar");
        assert!(kinds(&b.fader(MTOUCH, 1, 40, true, Some(3), Some(&h), 1.7)).is_empty());
        assert_eq!(b.stats.rebound_dropped, 2);
    }

    #[test]
    fn an_external_edit_after_pickup_resets_it_but_the_lag_of_our_own_positions_does_not() {
        let mut b = present(Banks::new(Config::default()));
        let h0 = hint(0.5, 0.0);
        b.fader(MTOUCH, 1, 100, true, Some(1), Some(&h0), 1.0);
        assert_eq!(kinds(&b.fader(MTOUCH, 1, 150, true, Some(1), Some(&h0), 1.1)).len(), 1, "picked up (crossed 0.5)");
        // The context 0.3 s after our position still says 0.5: our own lag, not an edit.
        let lag = hint(0.5, 1.4);
        assert_eq!(kinds(&b.fader(MTOUCH, 1, 160, true, Some(1), Some(&lag), 1.5)).len(), 1);
        assert_eq!(b.stats.pickup_resets, 0);
        // A context 2 s after our last position (at 1.5) reporting 0.1 while we sit at 0.63: edited elsewhere.
        let edit = hint(0.1, 3.6);
        let o = b.fader(MTOUCH, 1, 161, true, Some(1), Some(&edit), 3.7);
        assert!(kinds(&o).is_empty(), "the pickup is reset: nothing goes out until the strip reaches 0.1 again");
        assert_eq!(bars(&o), vec![true]);
        assert_eq!(b.stats.pickup_resets, 1);
        assert!(b.log.last().unwrap().contains("edited elsewhere"));
        assert_eq!(kinds(&b.fader(MTOUCH, 1, 20, true, Some(1), Some(&edit), 3.8)).len(), 1, "crossing 0.1 picks it up again");
    }

    /// Review (PR #7): continuous reports do not restart the detection window; a console that follows the strip
    /// (with its refresh lag) is confirmed by the positions in effect when it was read.
    #[test]
    fn continuous_reports_neither_hide_an_external_edit_nor_cause_a_false_reset() {
        let mut b = present(Banks::new(Config::default()));
        let h0 = hint(0.0, 0.0);
        b.fader(MTOUCH, 1, 0, true, Some(1), Some(&h0), 1.0);
        assert_eq!(kinds(&b.fader(MTOUCH, 1, 10, true, Some(1), Some(&h0), 1.1)).len(), 1, "picked up at the level");
        // The console follows with up to a second of lag: every 100 ms a report, every second a context showing the
        // value from about 0.9 s before. No reset.
        let mut t = 1.2;
        let mut v = 10u8;
        let mut hint_at = 1.0;
        let mut lagging = h0.clone();
        for _ in 0..40 {
            if t - hint_at >= 1.0 {
                hint_at = t;
                let seen = (v as f64 - 9.0 * 2.0).max(0.0) / 255.0;  // the value of ~0.9 s ago
                lagging = hint(seen, hint_at);
            }
            let o = b.fader(MTOUCH, 1, v, true, Some(1), Some(&lagging), t);
            assert_eq!(kinds(&o).len(), 1, "t={t}: a following console never resets the pickup");
            v = v.saturating_add(2);
            t += 0.1;
        }
        assert_eq!(b.stats.pickup_resets, 0);
        // Now the console stops following: contexts keep reporting 0.1 while the strip moves on at 100 ms.
        let stuck = |at: f64| hint(0.1, at);
        let mut reset_at = None;
        let start = t;
        for i in 0..40 {
            let h = stuck(start + (i as f64 * 0.1).floor());  // a context per second, all saying 0.1
            let o = b.fader(MTOUCH, 1, v, true, Some(1), Some(&h), t);
            if kinds(&o).is_empty() && reset_at.is_none() {
                reset_at = Some(t);
            }
            v = v.saturating_add(1);
            t += 0.1;
        }
        assert_eq!(b.stats.pickup_resets, 1, "continuous reports did not keep the window open");
        assert!(reset_at.map(|r| r - start <= 2.5).unwrap_or(false), "reset within a couple of refreshes: {reset_at:?}");
    }

    #[test]
    fn a_lost_device_lifts_its_faders_and_a_missed_lift_is_ended_locally() {
        let mut b = present(Banks::new(Config::default()));
        let h = hint(0.0, 0.0);
        b.fader(MTOUCH, 1, 0, true, Some(1), Some(&h), 1.0);
        b.fader(MPLAY_LEFT, 3, 0, true, Some(1), Some(&h), 1.0);
        assert_eq!(b.touched_count(), 2);
        let outs = b.disconnect("mplay");
        assert_eq!(outs, vec![Out::Fader { dev: "mplay", control: "MPF3", target: fader_target(203), kind: CtlKind::Touch { down: false } }]);
        assert_eq!(b.touched_count(), 1, "the M-Touch's touch survives the M-Play's loss");
        let o = b.tick(Some(1), 40.0);
        assert_eq!(kinds(&o), vec![CtlKind::Touch { down: false }], "touched for more than max_touch: ended locally");
        assert_eq!(b.stats.forced_lifts, 2);
        assert_eq!(b.touched_count(), 0);
    }

    #[test]
    fn mplay_pressure_buttons_are_one_down_and_one_up_and_mplay_faders_map_by_id() {
        let mut b = present(Banks::new(Config::default()));
        let d = b.pressure("mplay", 0x9205, 80, true);
        assert_eq!(d, Some(Out::Button { dev: "mplay", control: "MPA1", target: ex(101), kind: CtlKind::Btn { down: true } }), "the first touched report is the down");
        assert_eq!(b.pressure("mplay", 0x9205, 200, true), None, "a pressure change while held is nothing");
        assert_eq!(b.pressure("mplay", 0x9205, 0, false), Some(Out::Button { dev: "mplay", control: "MPA1", target: ex(101), kind: CtlKind::Btn { down: false } }), "the release report is the up");
        assert_eq!(b.pressure("mplay", 0x9205, 0, false), None, "a second release report is nothing");
        assert_eq!(b.pressure("mtouch", 0x4205, 50, true), None, "the M-Touch's PFD pressure key is unmapped (counted)");
        assert_eq!(b.stats.unmapped_keys, 1);
        assert_eq!(playback_fader_of(0x9213), Some((MPLAY_LEFT, 2)));
        assert_eq!(b.fader_executor(MPLAY_LEFT, 2), (202, None));
        assert_eq!(Banks::fader_bar_id(MPLAY_LEFT, 2), 0x9213);
        assert_eq!(Banks::fader_bar_id(MTOUCH, 10), 0x4293);
    }

    #[test]
    fn the_pickup_bar_lights_the_span_between_the_strip_and_the_level() {
        let data = Banks::pickup_bar(0.0, 0.5);  // LEDs 0..=5 (round(4.5) = 5 in Rust: away from zero)
        let word = |i: usize| u32::from_le_bytes([data[i * 3], data[i * 3 + 1], data[i * 3 + 2], 0]);
        assert_eq!(word(0) & 0x3FF, 0b11_1111, "green lane: LEDs 0..5");
        assert_eq!(word(1) & 0x3FF, 0b11_1111, "red lane too (yellow)");
        assert_eq!(word(2), 0, "no blue");
        assert_ne!(word(0) & crate::mtouch::BAR_BLINK, 0, "blinking");
        let one = Banks::pickup_bar(1.0, 1.0);
        assert_eq!(u32::from_le_bytes([one[0], one[1], one[2], 0]) & 0x3FF, 1 << 9, "at the level: the top LED alone");
    }

    #[test]
    fn dedicated_keys_press_the_profiles_executor_only_and_are_bound() {
        let mut off = present(Banks::new(Config::default()));
        assert_eq!(off.key_on("mtouch", KEY_GO, true), None, "no profile: unmapped");
        assert!(!off.targets().ex.contains(&290));
        let mut b = present(Banks::new(Config { go_executor: Some(290), pause_executor: Some(291), ..Config::default() }));
        assert!(b.targets().ex.contains(&290) && b.targets().ex.contains(&291), "the profile's executors are bound with the windows");
        let d = b.key_on("mplay", KEY_GO, true);
        assert_eq!(d, Some(Out::Button { dev: "mplay", control: "Go", target: ex(290), kind: CtlKind::Btn { down: true } }));
        b.key(0x4111, true);
        let u = b.key_on("mplay", KEY_GO, false);
        assert_eq!(u, Some(Out::Button { dev: "mplay", control: "Go", target: ex(290), kind: CtlKind::Btn { down: false } }), "never banked");
        assert_eq!(b.key_on("mtouch", KEY_PAUSE, true).map(|o| matches!(o, Out::Button { target, .. } if target == ex(291))), Some(true));
        assert_eq!(b.stats.dedicated, 2);
        assert_eq!(b.key_on("mtouch", 0x5502, true), None, "SELECT stays unmapped");
    }
}
