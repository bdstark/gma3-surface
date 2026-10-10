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
//!   * the playback FADERS are not operated here (KB-22: pickup against the executor's level, the configured fader
//!     function); their reports are counted `ignored_faders`. Buttons go out as `btn` events on the executor's
//!     `key` element; what the console does with them is the plugin's console backend (KB-22; the fake backend
//!     records them, the console backend refuses them `unsupported` until then).
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
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: true, mode: PageMode::Follow, rows: Rows { key_a: 101, fader: 201, key_b: 301 }, ext_row: 401, mplay: true }
    }
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
    /// Playback fader reports (not operated here: KB-22).
    pub ignored_faders: u64,
    /// Keys of neither a bank nor a playback button (command keys, MF keys, pressure keys).
    pub unmapped_keys: u64,
}

/// One control event for the link, or a bank change.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Button { dev: &'static str, control: &'static str, target: CtlTarget, kind: CtlKind },
    Bank { section: usize, bank: u32 },
}

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

pub struct Banks {
    pub cfg: Config,
    pub sections: Vec<Section>,
    /// Targets of the buttons currently down, by (device, control): what the release goes to.
    held: BTreeMap<(&'static str, &'static str), CtlTarget>,
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
        Banks { cfg, sections, held: BTreeMap::new(), rebind: true, stats: Stats::default(), log: Vec::new() }
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
    /// deduplicated. The plugin bounds the list (96); the M-Touch (30) and the M-Play (60) fit together.
    pub fn targets(&self) -> BindSpec {
        let mut ex: Vec<u32> = self.sections.iter().filter(|s| s.enabled && s.present).flat_map(|s| s.window()).collect();
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
    /// go out as `btn` events with a frozen target; everything else is counted.
    pub fn key(&mut self, id: u16, down: bool) -> Option<Out> {
        if let Some((section, delta)) = bank_key_of(id) {
            if down {
                return self.bank_step(section, delta);
            }
            return None;
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

    /// A playback fader report: not operated here (KB-22), counted.
    pub fn fader(&mut self) {
        self.stats.ignored_faders += 1;
    }

    /// The device went away: every button it held is released against its frozen target.
    pub fn disconnect(&mut self, dev: &'static str) -> Vec<Out> {
        let keys: Vec<(&'static str, &'static str)> = self.held.keys().filter(|(d, _)| *d == dev).copied().collect();
        let mut outs = Vec::new();
        for k in keys {
            if let Some(t) = self.held.remove(&k) {
                self.log.push(format!("{}/{}: {} lost while held; released {}", k.0, k.1, dev, describe(&t)));
                outs.push(Out::Button { dev: k.0, control: k.1, target: t, kind: CtlKind::Btn { down: false } });
            }
        }
        outs
    }

    pub fn held_count(&self) -> usize {
        self.held.len()
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
        format!("banks: {} {} held={} changes={} edge={} downs={} ups={} frozen={} ignored_faders={} unmapped={}", if self.cfg.enabled { mode } else { "off".into() }, secs.join(" "), self.held.len(), self.stats.bank_changes, self.stats.bank_edge, self.stats.downs, self.stats.ups, self.stats.frozen_releases, self.stats.ignored_faders, self.stats.unmapped_keys)
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
    fn unmapped_keys_and_faders_are_counted_not_dispatched() {
        let mut b = present(Banks::new(Config::default()));
        assert_eq!(b.key(0x5513, true), None);
        assert_eq!(b.key(0x4205, true), None);
        assert_eq!(b.stats.unmapped_keys, 2);
        b.fader();
        assert_eq!(b.stats.ignored_faders, 1);
    }
}
