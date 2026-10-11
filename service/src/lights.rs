//! KB-23: parameter colours and playback feedback on the M-Touch / M-Play lights (keys and bars). Pure: a
//! `Scene` (the fresh context body, the parameter strips, the playback controls the banks map right now) is
//! rendered to the device writes that changed, rate-limited and coalesced per flush; nothing here touches
//! input or release processing, which the loop handles before rendering.
//!
//! What is lit, and from what (KEYBOARD.md "KB-23"):
//! - Colours come from the context's VERIFIED metadata (vendored feedback 0.6.0): a slot's feature group /
//!   feature / activation group names and the definition's colour (`fg`, `fn`, `ag`, `col`), an executor's
//!   appearance background (`col`). A translated band label is never a colour source. The scheme for a
//!   parameter identifies the parameter (red = the red component); it never claims the fixture's emitted light.
//! - The M-Touch's four parameter strips are BLUE-ONLY hardware (key 0x61n1, bar 0x61n2): the parameter colour
//!   cannot be shown there; it is carried textually (`describe`) and rendered on RGB-capable bars (none of the
//!   shipped default layouts puts a parameter on one; the scheme is ready for KB-24's optional M-Play strips).
//!   The blue bar shows the VALUE: n of 10 LEDs for a single, completely read value; the two end LEDs blinking
//!   for MIXED values (never a fabricated single level); a blinking level for an incompletely read value; off
//!   for empty, none or unavailable. The strip's key is lit while the slot is adjustable, fast-blinking while touched.
//! - Playback: the single-colour (blue) PFA/PFB keys show ASSIGNED (on), RUNNING (slow blink) and PRESSED (fast
//!   blink); the red/green pressure keys (M-Touch PFD, every M-Play button) show ASSIGNED red, RUNNING green,
//!   PRESSED fast-blinking in the current colour; the RGB bar shows the configured fader function's LEVEL in the
//!   assignment colour (the appearance background reduced to the bar's 3-bit palette, green when the object has
//!   no usable appearance). The pickup span (KB-22, yellow blinking) keeps the bar while a touched fader waits.
//! - STALE (no fresh, known context: unpaired, link down, plugin silent for 1.5 s, generation unknown): every
//!   control that was lit from the last fresh context switches to the UNAVAILABLE pattern (bars: all ten LEDs
//!   blinking, red on RGB bars, blue on the parameter bars; red/green keys: red slow blink; blue keys: slow blink);
//!   controls that were dark stay dark. No level or colour survives into staleness.
//! - A rebind (a new generation, a bank change) with no context for it yet is the same unknown state: no old
//!   colour identifies a newly mapped control, because mapping and colour are always rendered from one context.
//! - Writes go out on change only and at most every `min_interval` (50 ms) per flush; a change that arrives
//!   between flushes is coalesced into the next one.

use crate::mtouch::{self, BarColour, LedKey, Model, Output};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    /// Minimum seconds between two flushes of device writes (coalescing bound).
    pub min_interval: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: true, min_interval: 0.05 }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub renders: u64,
    pub flushes: u64,
    pub writes: u64,
    /// Renders whose changes waited for the next flush (coalesced).
    pub coalesced: u64,
    /// Renders of the unavailable (stale) pattern.
    pub stale_renders: u64,
}

/// The kind of light a playback control has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    /// A single-colour (blue) LED key: M-Touch PFA/PFB.
    KeyBlue,
    /// A red/green pressure key: M-Touch PFD, every M-Play button.
    KeyRedGreen,
    /// An RGB 10-LED bar: M-Touch LEVEL PLAYBACK, M-Play FADER LEVEL.
    BarRgb,
}

/// One playback control the banks map right now, with the executor it means and its local state.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackControl {
    pub model: Model,
    pub id: u16,
    pub light: Light,
    pub control: &'static str,
    pub ex: u32,
    pub pg: Option<u32>,
    /// The button is down (its release pending) or, for a bar, its fader is touched.
    pub held: bool,
    /// KB-22: the fader is touched and waiting for pickup: the bar shows the span and is not ours.
    pub pickup_pending: bool,
    /// Hardware pass (local echo): a touched, picked-up playback fader's own position (0..1), shown as the level while
    /// touched so the bar follows the finger; the read-back takes over at the lift.
    pub echo: Option<f64>,
}

/// The M-Touch's parameter strips as the loop maps them.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamStrips {
    pub present: bool,
    pub base_slot: u8,
    pub touching: [bool; 4],
    /// Hardware pass (local echo): the predicted value of each touched strip (`Strips::echo`), shown instead of the
    /// read-back while touched so the bar follows the finger.
    pub echo: [Option<f64>; 4],
}

/// Everything one render needs.
pub struct Scene<'a> {
    /// The fresh, known context body (`Link::context_view`), None when it must be shown as unavailable.
    pub ctx: Option<&'a BTreeMap<String, Value>>,
    pub strips: Option<ParamStrips>,
    pub playback: &'a [PlaybackControl],
}

/// The identity colour of a parameter: a palette colour and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourSource {
    /// The attribute definition's own colour (`col`).
    Definition,
    /// The documented override table on the attribute's system name (ColorRGB_UV -> violet as magenta).
    NameOverride,
    /// The feature-group category scheme for non-colour parameters.
    Category,
    /// Nothing known: the neutral fallback.
    Neutral,
}

/// The bar's 3-bit palette colour nearest to "rrggbb"; None for text that is not six hex digits or a dark colour.
pub fn palette_of_hex(col: &str) -> Option<BarColour> {
    if col.len() != 6 || !col.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(&col[i..i + 2], 16).unwrap_or(0);
    let (r, g, b) = (ch(0), ch(2), ch(4));
    let max = r.max(g).max(b);
    if max < 0x40 {
        return None; // too dark to name
    }
    // A channel counts when it carries at least half of the brightest one (orange FF7F00 -> yellow, not red).
    let on = |c: u8| (c as u16) * 2 + 1 >= max as u16;
    Some(match (on(g), on(r), on(b)) {
        (true, false, false) => BarColour::Green,
        (false, true, false) => BarColour::Red,
        (false, false, true) => BarColour::Blue,
        (true, true, false) => BarColour::Yellow,
        (true, false, true) => BarColour::Cyan,
        (false, true, true) => BarColour::Magenta,
        _ => BarColour::White,
    })
}

/// The identity colour of an attribute slot from the context (`name`, `fg`, `col`), never from its label.
pub fn param_colour(slot: &Value) -> (BarColour, ColourSource) {
    let name = slot.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let fg = slot.get("fg").and_then(|v| v.as_str()).unwrap_or("");
    if fg == "Color" {
        // UV first: its definition colour, where one exists, is a visible stand-in anyway; violet is magenta here.
        if name.contains("UV") {
            return (BarColour::Magenta, ColourSource::NameOverride);
        }
        if let Some(c) = slot.get("col").and_then(|v| v.as_str()).and_then(palette_of_hex) {
            return (c, ColourSource::Definition);
        }
        let suffix = name.rsplit('_').next().unwrap_or("");
        let by_name = match suffix {
            "R" => Some(BarColour::Red),
            "G" => Some(BarColour::Green),
            "B" => Some(BarColour::Blue),
            "RY" | "A" | "Y" => Some(BarColour::Yellow), // amber / yellow components
            "W" | "WW" | "CW" => Some(BarColour::White),
            "C" => Some(BarColour::Cyan),
            "M" => Some(BarColour::Magenta),
            _ => None,
        };
        if let Some(c) = by_name {
            return (c, ColourSource::NameOverride);
        }
        return (BarColour::White, ColourSource::Category); // colour wheels, CTO, HSB: the group's colour
    }
    let cat = match fg {
        "Dimmer" => Some(BarColour::White),
        "Position" => Some(BarColour::Cyan),
        "Gobo" => Some(BarColour::Magenta),
        "Beam" => Some(BarColour::Yellow),
        "Focus" => Some(BarColour::Cyan),
        "Control" => Some(BarColour::Red),
        "Shapers" => Some(BarColour::Blue),
        "Video" => Some(BarColour::Green),
        _ => None,
    };
    match cat {
        Some(c) => (c, ColourSource::Category),
        None => (BarColour::Blue, ColourSource::Neutral),
    }
}

/// The assignment colour of an executor from its appearance (`col`); green when none is usable (neutral).
pub fn exec_colour(ex: &Value) -> (BarColour, ColourSource) {
    match ex.get("col").and_then(|v| v.as_str()).and_then(palette_of_hex) {
        Some(c) => (c, ColourSource::Definition),
        None => (BarColour::Green, ColourSource::Neutral),
    }
}

/// `n` of ten LEDs for a 0..1 level: at least one while the level is above zero, never ten below one.
pub fn leds_of(level: f64) -> u8 {
    let v = level.clamp(0.0, 1.0);
    if v <= 0.0 {
        0
    } else if v >= 1.0 {
        10
    } else {
        ((v * 10.0).round() as u8).clamp(1, 9)
    }
}

/// The "mixed values" pattern: the two end LEDs blinking (never a level).
pub fn mixed_bar(colour: BarColour) -> [u8; 9] {
    let mask: u16 = 0b10_0000_0001;
    let (g, r, b) = colour.lanes();
    let lane = |on: bool| if on { mask } else { 0 };
    mtouch::fader_bar(lane(g), lane(r), lane(b), (g, r, b))
}

/// The "unavailable" pattern: all ten LEDs blinking in `colour`.
pub fn stale_bar(colour: BarColour) -> [u8; 9] {
    mtouch::bar_level(10, colour, true)
}

/// What one playback control's light shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Dark,
    Assigned,
    Running,
    Pressed { running: bool },
    Unavailable,
}

fn exec_entry<'a>(ctx: &'a BTreeMap<String, Value>, ex: u32, pg: Option<u32>) -> Option<&'a Value> {
    ctx.get("ex")?.as_array()?.iter().find(|x| x.get("n").and_then(|n| n.as_u64()) == Some(ex as u64) && x.get("pg").and_then(|p| p.as_u64()).map(|p| p as u32) == pg)
}

fn slot_entry<'a>(ctx: &'a BTreeMap<String, Value>, slot: u8) -> Option<&'a Value> {
    ctx.get("slots")?.as_array()?.iter().find(|x| x.get("n").and_then(|n| n.as_u64()) == Some(slot as u64))
}

fn is_target(x: &Value) -> bool {
    x.get("tgt").and_then(|v| v.as_u64()) == Some(1)
}

fn active(x: &Value) -> Option<bool> {
    match x.get("act") {
        Some(Value::Number(n)) => Some(n.as_u64() == Some(1)),
        _ => None,
    }
}

fn level(x: &Value) -> Option<f64> {
    x.get("lvl").and_then(|v| v.as_f64()).map(|l| (l / 100.0).clamp(0.0, 1.0))
}

pub fn playback_state(x: Option<&Value>, held: bool) -> PlaybackState {
    let Some(x) = x else { return PlaybackState::Dark };
    if !is_target(x) {
        return PlaybackState::Dark;
    }
    let running = active(x) == Some(true);
    if held {
        PlaybackState::Pressed { running }
    } else if running {
        PlaybackState::Running
    } else {
        PlaybackState::Assigned
    }
}

/// The blue single-colour key for a playback state.
pub fn key_blue(state: PlaybackState) -> LedKey {
    match state {
        PlaybackState::Dark => LedKey::off(),
        PlaybackState::Assigned => LedKey::off().blue(),
        PlaybackState::Running => LedKey::off().blue().slow(),
        PlaybackState::Pressed { .. } => LedKey::off().blue().fast(),
        PlaybackState::Unavailable => LedKey::off().blue().slow(),
    }
}

/// The red/green pressure key for a playback state.
pub fn key_red_green(state: PlaybackState) -> LedKey {
    match state {
        PlaybackState::Dark => LedKey::off(),
        PlaybackState::Assigned => LedKey::off().red(),
        PlaybackState::Running => LedKey::off().green(),
        PlaybackState::Pressed { running: true } => LedKey::off().green().fast(),
        PlaybackState::Pressed { running: false } => LedKey::off().red().fast(),
        PlaybackState::Unavailable => LedKey::off().red().slow(),
    }
}

/// What a parameter strip's blue bar shows for its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueState {
    Dark,
    /// A single, completely read value: n LEDs.
    Level(u8),
    /// A value read from the readable fixtures only (incomplete): n LEDs blinking.
    LevelIncomplete(u8),
    /// Different values on the selected fixtures: the end LEDs blink.
    Mixed,
}

pub fn value_state(slot: Option<&Value>) -> ValueState {
    let Some(s) = slot else { return ValueState::Dark };
    if s.get("kind").and_then(|v| v.as_str()) != Some("attribute") {
        return ValueState::Dark;
    }
    match s.get("val").and_then(|v| v.as_str()).unwrap_or("none") {
        "mixed" => ValueState::Mixed,
        "value" => {
            let abs = s.get("abs").and_then(|v| v.as_f64()).map(|a| (a / 100.0).clamp(0.0, 1.0));
            let complete = s.get("vc").and_then(|v| v.as_u64()) == Some(1);
            match abs {
                Some(a) if complete => ValueState::Level(leds_of(a)),
                Some(a) => ValueState::LevelIncomplete(leds_of(a)),
                None => ValueState::Dark,
            }
        }
        _ => ValueState::Dark, // none, empty, unavailable, unsupported
    }
}

/// Whether the slot is something the strip adjusts right now (its key lights).
pub fn slot_adjustable(slot: Option<&Value>) -> bool {
    let Some(s) = slot else { return false };
    s.get("kind").and_then(|v| v.as_str()) == Some("attribute") && matches!(s.get("avail").and_then(|v| v.as_str()), Some("available") | Some("mixed"))
}

pub fn value_bar(state: ValueState, colour: BarColour) -> [u8; 9] {
    match state {
        ValueState::Dark => [0u8; 9],
        ValueState::Level(n) => mtouch::bar_level(n, colour, false),
        ValueState::LevelIncomplete(n) => mtouch::bar_level(n, colour, true),
        ValueState::Mixed => mixed_bar(colour),
    }
}

/// M-Touch base-channel ids: key 0x61n1, bar 0x61n2 for strip n (1..4).
pub const fn param_key_id(n: u8) -> u16 {
    0x6101 + 0x10 * (n as u16)
}
pub const fn param_bar_id(n: u8) -> u16 {
    0x6102 + 0x10 * (n as u16)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Want {
    Key(u16),
    Bar([u8; 9]),
}

impl Want {
    fn output(&self, id: u16) -> Output {
        match self {
            Want::Key(value) => Output::LedKey { id, value: *value },
            Want::Bar(data) => Output::FaderBar { id, data: *data },
        }
    }
}

pub struct Lights {
    pub cfg: Config,
    pub stats: Stats,
    /// What each light last showed on the device (as written by us or noted from the banks' bar writes).
    last: BTreeMap<(Model, u16), Want>,
    /// The latest computed state, flushed at the next opportunity.
    want: BTreeMap<(Model, u16), Want>,
    /// Lights lit from the last fresh context: these show the unavailable pattern while stale.
    lit: BTreeMap<(Model, u16), BarColour>,
    last_flush: Option<f64>,
    /// The textual state of every control (`describe`), rebuilt each render.
    pub lines: Vec<String>,
    /// Hardware pass: a fresh context exists but belongs to the previous binding (a bank change in flight): set by the
    /// caller before each render (`set_rebinding`), and since when.
    rebinding: bool,
    rebind_since: Option<f64>,
}

/// Hardware pass: how long a rebind (fresh context, previous binding) is shown dark before it counts as stale. A bank
/// change completes in a few hundred milliseconds; showing the unavailable pattern for that gap read as an error.
pub const REBIND_GRACE: f64 = 1.0;

impl Lights {
    pub fn new(cfg: Config) -> Lights {
        Lights { cfg, stats: Stats::default(), last: BTreeMap::new(), want: BTreeMap::new(), lit: BTreeMap::new(), last_flush: None, lines: Vec::new(), rebinding: false, rebind_since: None }
    }

    /// Hardware pass: whether the missing context is a rebind in flight (the link has a fresh context of the previous
    /// binding) rather than staleness. Call before `render`.
    pub fn set_rebinding(&mut self, rebinding: bool) {
        self.rebinding = rebinding;
    }

    /// Tests: pretend the current rebind started `secs` earlier (the loop's clock is real time).
    #[cfg(test)]
    pub fn age_rebind(&mut self, secs: f64) {
        if let Some(t) = self.rebind_since.as_mut() {
            *t -= secs;
        }
    }

    /// A write another part of the loop made to a light (the banks' pickup bar): the cache follows it.
    pub fn note_external(&mut self, model: Model, out: &Output) {
        match out {
            Output::LedKey { id, value } => {
                self.last.insert((model, *id), Want::Key(*value));
            }
            Output::FaderBar { id, data } => {
                self.last.insert((model, *id), Want::Bar(*data));
            }
            Output::PageDisplay { .. } => {}
        }
    }

    /// The device was (re)opened: every light of it is rewritten.
    pub fn invalidate(&mut self, model: Model) {
        self.last.retain(|(m, _), _| *m != model);
        self.last_flush = None;
    }

    /// Computes every light from the scene and returns the writes that are due (change-only, rate-limited).
    pub fn render(&mut self, scene: &Scene<'_>, now: f64) -> Vec<(Model, Output)> {
        if !self.cfg.enabled {
            return Vec::new();
        }
        self.stats.renders += 1;
        let mut want: BTreeMap<(Model, u16), Want> = BTreeMap::new();
        let mut lit: BTreeMap<(Model, u16), BarColour> = BTreeMap::new();
        let mut lines = Vec::new();
        let stale = scene.ctx.is_none();
        // A rebind in flight is shown dark (nothing old identifies the new mapping, nothing claims staleness) for up to
        // REBIND_GRACE; after that, or for any other missing context, the unavailable pattern.
        if stale && self.rebinding {
            self.rebind_since.get_or_insert(now);
        } else {
            self.rebind_since = None;
        }
        let dark_rebind = stale && self.rebind_since.map(|t| now - t < REBIND_GRACE).unwrap_or(false);
        if stale && !dark_rebind {
            self.stats.stale_renders += 1;
        }
        // Parameter strips (M-Touch base channels, blue only).
        if let Some(st) = &scene.strips {
            if st.present {
                for n in 1..=4u8 {
                    let slot_no = st.base_slot.clamp(1, 5) + n - 1;
                    let key_id = param_key_id(n);
                    let bar_id = param_bar_id(n);
                    let touching = st.touching[(n - 1) as usize];
                    match scene.ctx {
                        Some(ctx) => {
                            let slot = slot_entry(ctx, slot_no);
                            let adjustable = slot_adjustable(slot);
                            let vs = value_state(slot);
                            let key = if !adjustable { LedKey::off() } else if touching { LedKey::off().blue().fast() } else { LedKey::off().blue() };
                            want.insert((Model::MTouch, key_id), Want::Key(key.value()));
                            // Local echo while touched: the predicted value, never over a mixed selection.
                            let shown = match st.echo[(n - 1) as usize] {
                                Some(e) if touching && vs != ValueState::Mixed => ValueState::Level(leds_of(e)),
                                _ => vs,
                            };
                            want.insert((Model::MTouch, bar_id), Want::Bar(value_bar(shown, BarColour::Blue)));
                            if adjustable {
                                lit.insert((Model::MTouch, key_id), BarColour::Blue);
                            }
                            if vs != ValueState::Dark {
                                lit.insert((Model::MTouch, bar_id), BarColour::Blue);
                            }
                            lines.push(describe_slot(n, slot_no, slot, vs, touching));
                        }
                        None => {
                            // Unavailable: what was lit blinks, what was dark stays dark (a rebind in flight: all dark).
                            let key_lit = self.lit.contains_key(&(Model::MTouch, key_id));
                            let bar_lit = self.lit.contains_key(&(Model::MTouch, bar_id));
                            want.insert((Model::MTouch, key_id), Want::Key(if key_lit && !dark_rebind { LedKey::off().blue().slow().value() } else { 0 }));
                            want.insert((Model::MTouch, bar_id), Want::Bar(if bar_lit && !dark_rebind { stale_bar(BarColour::Blue) } else { [0u8; 9] }));
                            if key_lit {
                                lit.insert((Model::MTouch, key_id), BarColour::Blue);
                            }
                            if bar_lit {
                                lit.insert((Model::MTouch, bar_id), BarColour::Blue);
                            }
                            if dark_rebind {
                                lines.push(format!("strip{n} slot{slot_no}: rebinding (the new binding's context has not arrived), dark"));
                            } else {
                                lines.push(format!("strip{n} slot{slot_no}: unavailable (no fresh context){}", if key_lit || bar_lit { ", shown as unavailable" } else { ", dark" }));
                            }
                        }
                    }
                }
            }
        }
        // Playback controls.
        for c in scene.playback {
            let key = (c.model, c.id);
            match scene.ctx {
                Some(ctx) => {
                    let x = exec_entry(ctx, c.ex, c.pg);
                    let state = playback_state(x, c.held);
                    match c.light {
                        Light::KeyBlue => {
                            want.insert(key, Want::Key(key_blue(state).value()));
                        }
                        Light::KeyRedGreen => {
                            want.insert(key, Want::Key(key_red_green(state).value()));
                        }
                        Light::BarRgb => {
                            if c.pickup_pending {
                                // KB-22: the pickup span owns the bar until the lift or the pickup.
                                lines.push(format!("{} {} ex{}{}: pickup pending (the span is shown)", c.model.name(), c.control, c.ex, pg_text(c.pg)));
                                if let Some(x) = x {
                                    lit.insert(key, exec_colour(x).0);
                                }
                                continue;
                            }
                            let (colour, _) = x.map(exec_colour).unwrap_or((BarColour::Green, ColourSource::Neutral));
                            // Local echo: a touched, picked-up fader's own position is the level it is setting.
                            let lvl = x.filter(|x| is_target(x)).and_then(|x| c.echo.or_else(|| level(x)));
                            let data = match lvl {
                                Some(l) => mtouch::bar_level(leds_of(l), colour, false),
                                None => [0u8; 9],
                            };
                            want.insert(key, Want::Bar(data));
                            if lvl.is_some() {
                                lit.insert(key, colour);
                            }
                        }
                    }
                    if state != PlaybackState::Dark && c.light != Light::BarRgb {
                        lit.insert(key, BarColour::Red);
                    }
                    if describes(c) {
                        lines.push(describe_exec(c, x, state));
                    }
                }
                None => {
                    let was_lit = self.lit.get(&key).copied();
                    let shown_lit = if dark_rebind { None } else { was_lit };
                    let w = match (c.light, shown_lit) {
                        (_, None) => match c.light {
                            Light::BarRgb => Want::Bar([0u8; 9]),
                            _ => Want::Key(0),
                        },
                        (Light::KeyBlue, Some(_)) => Want::Key(key_blue(PlaybackState::Unavailable).value()),
                        (Light::KeyRedGreen, Some(_)) => Want::Key(key_red_green(PlaybackState::Unavailable).value()),
                        (Light::BarRgb, Some(_)) => Want::Bar(stale_bar(BarColour::Red)),
                    };
                    if c.pickup_pending && c.light == Light::BarRgb {
                        if let Some(col) = was_lit {
                            lit.insert(key, col);
                        }
                        continue;
                    }
                    want.insert(key, w);
                    if let Some(col) = was_lit {
                        lit.insert(key, col);
                    }
                    if describes(c) {
                        if dark_rebind {
                            lines.push(format!("{} {} ex{}{}: rebinding (the new binding's context has not arrived), dark", c.model.name(), c.control, c.ex, pg_text(c.pg)));
                        } else {
                            lines.push(format!("{} {} ex{}{}: unavailable (no fresh context){}", c.model.name(), c.control, c.ex, pg_text(c.pg), if was_lit.is_some() { ", shown as unavailable" } else { ", dark" }));
                        }
                    }
                }
            }
        }
        self.lit = lit;
        self.lines = lines;
        self.want = want;
        // Rate limit: flush at most every min_interval; a change in between waits (coalesced).
        let changed = self.want.iter().any(|(k, w)| self.last.get(k) != Some(w));
        if !changed {
            return Vec::new();
        }
        if let Some(t) = self.last_flush {
            if now - t < self.cfg.min_interval {
                self.stats.coalesced += 1;
                return Vec::new();
            }
        }
        self.last_flush = Some(now);
        self.stats.flushes += 1;
        let mut writes = Vec::new();
        for (k, w) in &self.want {
            if self.last.get(k) != Some(w) {
                self.last.insert(*k, w.clone());
                writes.push((k.0, w.output(k.1)));
                self.stats.writes += 1;
            }
        }
        writes
    }

    pub fn summary(&self) -> String {
        format!("lights: enabled={} renders={} flushes={} writes={} coalesced={} stale_renders={} lit={}", self.cfg.enabled, self.stats.renders, self.stats.flushes, self.stats.writes, self.stats.coalesced, self.stats.stale_renders, self.lit.len())
    }
}

/// One textual line per executor row control: the keys and the bar; the M-Touch PFD shares the fader row's
/// executor with the bar (its line) and adds none.
fn describes(c: &PlaybackControl) -> bool {
    !(c.light == Light::KeyRedGreen && c.control.starts_with("PB"))
}

fn pg_text(pg: Option<u32>) -> String {
    pg.map(|p| format!("@p{p}")).unwrap_or_default()
}

/// The textual state of a parameter strip (so colour is never the only cue).
fn describe_slot(n: u8, slot_no: u8, slot: Option<&Value>, vs: ValueState, touching: bool) -> String {
    let Some(s) = slot else { return format!("strip{n} slot{slot_no}: not in the context") };
    let str_of = |k: &str| s.get(k).and_then(|v| v.as_str()).unwrap_or("");
    if str_of("kind") != "attribute" {
        return format!("strip{n} slot{slot_no}: {} slot, dark", if str_of("kind").is_empty() { "unknown" } else { str_of("kind") });
    }
    let (colour, source) = param_colour(s);
    let value = match vs {
        ValueState::Dark => format!("{}", str_of("val")),
        ValueState::Level(k) => format!("{}% single ({k}/10)", s.get("abs").and_then(|v| v.as_f64()).unwrap_or(0.0).round()),
        ValueState::LevelIncomplete(k) => format!("{}% incomplete read ({k}/10 blinking)", s.get("abs").and_then(|v| v.as_f64()).unwrap_or(0.0).round()),
        ValueState::Mixed => "mixed (end LEDs blinking)".to_string(),
    };
    format!(
        "strip{n} slot{slot_no}: {} \"{}\" {}/{}{} colour={:?} ({:?}; the strip's bar is blue-only) avail={} value={}{}",
        str_of("name"), str_of("label"), str_of("fg"), str_of("fn"),
        if str_of("ag").is_empty() { String::new() } else { format!(" [{}]", str_of("ag")) },
        colour, source, str_of("avail"), value, if touching { " touched" } else { "" }
    )
}

fn describe_exec(c: &PlaybackControl, x: Option<&Value>, state: PlaybackState) -> String {
    let head = format!("{} {} ex{}{}", c.model.name(), c.control, c.ex, pg_text(c.pg));
    let Some(x) = x else { return format!("{head}: not in the context, dark") };
    let str_of = |k: &str| x.get(k).and_then(|v| v.as_str()).unwrap_or("");
    if !is_target(x) {
        return format!("{head}: {}, dark", if x.get("empty").and_then(|v| v.as_u64()) == Some(1) { "empty".to_string() } else { format!("not a playback target{}", if str_of("why").is_empty() { String::new() } else { format!(" ({})", str_of("why")) }) });
    }
    let (colour, source) = exec_colour(x);
    let lvl = level(x).map(|l| format!("{}%", (l * 100.0).round())).unwrap_or_else(|| "unknown".into());
    let act = match active(x) {
        Some(true) => "running",
        Some(false) => "idle",
        None => "activity unknown",
    };
    format!("{head}: {} \"{}\" {}/{} level={lvl} {act} colour={:?} ({:?}) -> {:?}", str_of("cls"), str_of("name"), str_of("kp"), str_of("fd"), colour, source, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(slots: Value, ex: Value) -> BTreeMap<String, Value> {
        let mut m = BTreeMap::new();
        m.insert("slots".into(), slots);
        m.insert("ex".into(), ex);
        m
    }
    fn pb(model: Model, id: u16, light: Light, control: &'static str, ex: u32, held: bool, pickup: bool) -> PlaybackControl {
        PlaybackControl { model, id, light, control, ex, pg: None, held, pickup_pending: pickup, echo: None }
    }
    fn bar_of(writes: &[(Model, Output)], id: u16) -> Option<[u8; 9]> {
        writes.iter().find_map(|(_, o)| match o { Output::FaderBar { id: i, data } if *i == id => Some(*data), _ => None })
    }
    fn key_of(writes: &[(Model, Output)], id: u16) -> Option<u16> {
        writes.iter().find_map(|(_, o)| match o { Output::LedKey { id: i, value } if *i == id => Some(*value), _ => None })
    }
    fn fast() -> Config {
        Config { enabled: true, min_interval: 0.0 }
    }

    #[test]
    fn colours_come_from_the_definition_then_the_name_then_the_category_never_the_label() {
        assert_eq!(palette_of_hex("ff0000"), Some(BarColour::Red));
        assert_eq!(palette_of_hex("ff7f00"), Some(BarColour::Yellow), "orange reduces to the yellow lanes");
        assert_eq!(palette_of_hex("00ffff"), Some(BarColour::Cyan));
        assert_eq!(palette_of_hex("202020"), None, "too dark to name");
        assert_eq!(palette_of_hex("zz0000"), None);
        let r = json!({ "name": "ColorRGB_R", "label": "Blau", "fg": "Color", "col": "ff0000" });
        assert_eq!(param_colour(&r), (BarColour::Red, ColourSource::Definition), "the label says blue; the definition wins");
        let g = json!({ "name": "ColorRGB_G", "label": "G", "fg": "Color" });
        assert_eq!(param_colour(&g), (BarColour::Green, ColourSource::NameOverride), "no colour defined: the system name's component");
        let uv = json!({ "name": "ColorRGB_UV", "fg": "Color", "col": "000000" });
        assert_eq!(param_colour(&uv), (BarColour::Magenta, ColourSource::NameOverride), "UV is the documented violet stand-in");
        let amber = json!({ "name": "ColorRGB_RY", "fg": "Color" });
        assert_eq!(param_colour(&amber).0, BarColour::Yellow);
        let dim = json!({ "name": "Dimmer", "fg": "Dimmer", "col": "000000" });
        assert_eq!(param_colour(&dim), (BarColour::White, ColourSource::Category), "a black definition colour is no colour: the category scheme");
        let pan = json!({ "name": "Pan", "fg": "Position" });
        assert_eq!(param_colour(&pan), (BarColour::Cyan, ColourSource::Category));
        let odd = json!({ "name": "Weird" });
        assert_eq!(param_colour(&odd), (BarColour::Blue, ColourSource::Neutral));
        assert_eq!(exec_colour(&json!({ "col": "ff7f00" })), (BarColour::Yellow, ColourSource::Definition));
        assert_eq!(exec_colour(&json!({})), (BarColour::Green, ColourSource::Neutral));
    }

    #[test]
    fn value_indicators_never_fabricate_a_single_level() {
        assert_eq!(leds_of(0.0), 0);
        assert_eq!(leds_of(0.01), 1, "above zero shows");
        assert_eq!(leds_of(0.5), 5);
        assert_eq!(leds_of(0.99), 9, "below full never shows full");
        assert_eq!(leds_of(1.0), 10);
        let single = json!({ "n": 1, "kind": "attribute", "val": "value", "abs": 50.0, "vc": 1, "avail": "available" });
        assert_eq!(value_state(Some(&single)), ValueState::Level(5));
        let incomplete = json!({ "n": 1, "kind": "attribute", "val": "value", "abs": 50.0, "vc": 0, "avail": "available" });
        assert_eq!(value_state(Some(&incomplete)), ValueState::LevelIncomplete(5));
        let mixed = json!({ "n": 1, "kind": "attribute", "val": "mixed", "abs": 50.0, "vc": 1, "avail": "mixed" });
        assert_eq!(value_state(Some(&mixed)), ValueState::Mixed, "the first fixture's value is in abs but mixed is never a level");
        let empty = json!({ "n": 1, "kind": "attribute", "val": "empty", "avail": "available" });
        assert_eq!(value_state(Some(&empty)), ValueState::Dark);
        assert_eq!(value_state(Some(&json!({ "n": 1, "kind": "other" }))), ValueState::Dark);
        assert_eq!(value_state(None), ValueState::Dark);
        // the mixed pattern lights the two end LEDs blinking in the lane
        let m = mixed_bar(BarColour::Blue);
        let blue_word = u32::from_le_bytes([m[6], m[7], m[8], 0]);
        assert_eq!(blue_word & 0x3FF, 0b10_0000_0001);
        assert_ne!(blue_word & mtouch::BAR_BLINK, 0);
        assert_eq!(u32::from_le_bytes([m[0], m[1], m[2], 0]), 0, "no green on a blue-only bar");
    }

    #[test]
    fn parameter_strips_show_value_mixed_and_touch_on_the_blue_bar_and_key() {
        let c = ctx(json!([
            { "n": 1, "kind": "attribute", "name": "ColorRGB_R", "label": "R", "fg": "Color", "fn": "RGB", "col": "ff0000", "val": "value", "abs": 50.0, "vc": 1, "avail": "available" },
            { "n": 2, "kind": "attribute", "name": "ColorRGB_G", "fg": "Color", "val": "mixed", "abs": 20.0, "vc": 1, "avail": "mixed" },
            { "n": 3, "kind": "attribute", "name": "ColorRGB_B", "fg": "Color", "val": "none", "avail": "no-selection" },
            { "n": 4, "kind": "empty" }
        ]), json!([]));
        let mut l = Lights::new(fast());
        let strips = ParamStrips { present: true, base_slot: 1, touching: [false, false, true, false], echo: [None; 4] };
        let w = l.render(&Scene { ctx: Some(&c), strips: Some(strips), playback: &[] }, 1.0);
        assert_eq!(bar_of(&w, param_bar_id(1)), Some(mtouch::bar_level(5, BarColour::Blue, false)), "a single complete value is five blue LEDs");
        assert_eq!(key_of(&w, param_key_id(1)), Some(LedKey::off().blue().value()));
        assert_eq!(bar_of(&w, param_bar_id(2)), Some(mixed_bar(BarColour::Blue)));
        assert_eq!(bar_of(&w, param_bar_id(3)), Some([0u8; 9]), "no selection: dark");
        assert_eq!(key_of(&w, param_key_id(3)), Some(0), "not adjustable (touched or not): dark");
        assert_eq!(bar_of(&w, param_bar_id(4)), Some([0u8; 9]));
        assert!(l.lines.iter().any(|s| s.contains("ColorRGB_R \"R\" Color/RGB colour=Red (Definition") && s.contains("50% single (5/10)")), "{:?}", l.lines);
        assert!(l.lines.iter().any(|s| s.contains("mixed (end LEDs blinking)")));
        // touching the adjustable strip 1 fast-blinks its key; nothing else changes (one write)
        let strips2 = ParamStrips { present: true, base_slot: 1, touching: [true, false, false, false], echo: [None; 4] };
        let w2 = l.render(&Scene { ctx: Some(&c), strips: Some(strips2), playback: &[] }, 2.0);
        assert_eq!(w2.len(), 1);
        assert_eq!(key_of(&w2, param_key_id(1)), Some(LedKey::off().blue().fast().value()));
    }

    #[test]
    fn playback_lights_distinguish_assigned_running_pressed_and_level_colour() {
        let c = ctx(json!([]), json!([
            { "n": 101, "tgt": 1, "cls": "Sequence", "name": "A", "kp": "Go+", "fd": "Master", "lvl": 100, "act": 0, "col": "ff7f00" },
            { "n": 201, "tgt": 1, "cls": "Sequence", "name": "B", "kp": "Temp", "fd": "Master", "lvl": 50, "act": 1 },
            { "n": 301, "empty": 1, "tgt": 0 },
            { "n": 102, "tgt": 1, "act": 1 }
        ]));
        let controls = vec![
            pb(Model::MTouch, 0x4201, Light::KeyBlue, "PFA1", 101, false, false),
            pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 201, false, false),
            pb(Model::MTouch, 0x4205, Light::KeyRedGreen, "PFD1", 201, false, false),
            pb(Model::MTouch, 0x4202, Light::KeyBlue, "PFB1", 301, false, false),
            pb(Model::MTouch, 0x4211, Light::KeyBlue, "PFA2", 102, true, false),
            pb(Model::MPlay, 0x9205, Light::KeyRedGreen, "MPA1", 101, true, false),
            pb(Model::MPlay, 0x9203, Light::BarRgb, "MPF1", 101, false, false),
        ];
        let mut l = Lights::new(fast());
        let w = l.render(&Scene { ctx: Some(&c), strips: None, playback: &controls }, 1.0);
        assert_eq!(key_of(&w, 0x4201), Some(LedKey::off().blue().value()), "assigned, idle: on");
        assert_eq!(key_of(&w, 0x4202), Some(0), "empty: dark");
        assert_eq!(key_of(&w, 0x4211), Some(LedKey::off().blue().fast().value()), "pressed: fast blink");
        assert_eq!(key_of(&w, 0x4205), Some(LedKey::off().green().value()), "running on the red/green key: green");
        assert_eq!(key_of(&w, 0x9205), Some(LedKey::off().red().fast().value()), "pressed while idle: red fast");
        assert_eq!(bar_of(&w, 0x4203), Some(mtouch::bar_level(5, BarColour::Green, false)), "no appearance: the neutral green at the level");
        assert_eq!(bar_of(&w, 0x9203), Some(mtouch::bar_level(10, BarColour::Yellow, false)), "orange appearance: yellow lanes, full");
        assert!(l.lines.iter().any(|s| s.contains("M-Play MPF1 ex101: Sequence \"A\" Go+/Master level=100% idle colour=Yellow (Definition)")), "{:?}", l.lines);
        // the second render with the same scene writes nothing
        assert!(l.render(&Scene { ctx: Some(&c), strips: None, playback: &controls }, 2.0).is_empty());
    }

    #[test]
    fn stale_shows_the_unavailable_pattern_only_where_something_was_lit_and_keeps_no_level() {
        let c = ctx(json!([{ "n": 1, "kind": "attribute", "name": "Dimmer", "fg": "Dimmer", "val": "value", "abs": 80.0, "vc": 1, "avail": "available" }]),
                    json!([{ "n": 201, "tgt": 1, "lvl": 70, "act": 0, "col": "ff0000" }, { "n": 301, "empty": 1, "tgt": 0 }]));
        let controls = vec![
            pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 201, false, false),
            pb(Model::MTouch, 0x4205, Light::KeyRedGreen, "PFD1", 201, false, false),
            pb(Model::MTouch, 0x4202, Light::KeyBlue, "PFB1", 301, false, false),
        ];
        let strips = ParamStrips { present: true, base_slot: 1, touching: [false; 4], echo: [None; 4] };
        let mut l = Lights::new(fast());
        let _ = l.render(&Scene { ctx: Some(&c), strips: Some(strips.clone()), playback: &controls }, 1.0);
        let w = l.render(&Scene { ctx: None, strips: Some(strips.clone()), playback: &controls }, 2.0);
        assert_eq!(bar_of(&w, 0x4203), Some(stale_bar(BarColour::Red)), "the lit bar blinks red all over: no level survives");
        assert_eq!(key_of(&w, 0x4205), Some(LedKey::off().red().slow().value()));
        assert_eq!(bar_of(&w, param_bar_id(1)), Some(stale_bar(BarColour::Blue)));
        assert_eq!(key_of(&w, param_key_id(1)), Some(LedKey::off().blue().slow().value()));
        assert!(key_of(&w, 0x4202).is_none(), "the dark empty key was dark before and is not rewritten");
        assert!(bar_of(&w, param_bar_id(2)).is_none(), "an unlit strip bar stays dark without a write");
        assert!(l.lines.iter().any(|s| s.contains("PFD1 ex201: unavailable (no fresh context), shown as unavailable")), "{:?}", l.lines);
        assert_eq!(l.stats.stale_renders, 1);
        // the context comes back: levels and colours again
        let w3 = l.render(&Scene { ctx: Some(&c), strips: Some(strips), playback: &controls }, 3.0);
        assert_eq!(bar_of(&w3, 0x4203), Some(mtouch::bar_level(7, BarColour::Red, false)));
    }

    #[test]
    fn a_new_mapping_without_its_context_carries_no_old_colour() {
        let c = ctx(json!([]), json!([{ "n": 201, "tgt": 1, "lvl": 100, "act": 1, "col": "ff0000" }]));
        let before = vec![pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 201, false, false), pb(Model::MTouch, 0x4205, Light::KeyRedGreen, "PFD1", 201, false, false)];
        let mut l = Lights::new(fast());
        let _ = l.render(&Scene { ctx: Some(&c), strips: None, playback: &before }, 1.0);
        // a bank change: the strip now means 211; the context for the new binding has not arrived (ctx None)
        let after = vec![pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 211, false, false), pb(Model::MTouch, 0x4205, Light::KeyRedGreen, "PFD1", 211, false, false)];
        let w = l.render(&Scene { ctx: None, strips: None, playback: &after }, 2.0);
        assert_eq!(bar_of(&w, 0x4203), Some(stale_bar(BarColour::Red)), "unavailable, not 201's red level");
        // the new context lists 211 empty: both go dark; nothing of 201 remains
        let c2 = ctx(json!([]), json!([{ "n": 211, "empty": 1, "tgt": 0 }]));
        let w2 = l.render(&Scene { ctx: Some(&c2), strips: None, playback: &after }, 3.0);
        assert_eq!(bar_of(&w2, 0x4203), Some([0u8; 9]));
        assert_eq!(key_of(&w2, 0x4205), Some(0));
    }

    #[test]
    fn writes_are_rate_limited_and_coalesced_and_follow_external_bar_writes() {
        let c1 = ctx(json!([]), json!([{ "n": 201, "tgt": 1, "lvl": 10, "act": 0 }]));
        let c2 = ctx(json!([]), json!([{ "n": 201, "tgt": 1, "lvl": 20, "act": 0 }]));
        let c3 = ctx(json!([]), json!([{ "n": 201, "tgt": 1, "lvl": 30, "act": 0 }]));
        let controls = vec![pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 201, false, false)];
        let mut l = Lights::new(Config { enabled: true, min_interval: 0.05 });
        assert_eq!(l.render(&Scene { ctx: Some(&c1), strips: None, playback: &controls }, 1.00).len(), 1);
        assert!(l.render(&Scene { ctx: Some(&c2), strips: None, playback: &controls }, 1.01).is_empty(), "within the interval: held back");
        assert!(l.render(&Scene { ctx: Some(&c3), strips: None, playback: &controls }, 1.02).is_empty());
        let w = l.render(&Scene { ctx: Some(&c3), strips: None, playback: &controls }, 1.06);
        assert_eq!(bar_of(&w, 0x4203), Some(mtouch::bar_level(3, BarColour::Green, false)), "one write with the latest state");
        assert_eq!(l.stats.coalesced, 2);
        assert_eq!(l.stats.writes, 2);
        // the banks wrote the pickup span and then cleared the bar: the cache follows, so our level is rewritten
        let pending = vec![pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 201, true, true)];
        assert!(l.render(&Scene { ctx: Some(&c3), strips: None, playback: &pending }, 1.2).is_empty(), "the span owns the bar");
        l.note_external(Model::MTouch, &Output::FaderBar { id: 0x4203, data: [0u8; 9] });
        let w = l.render(&Scene { ctx: Some(&c3), strips: None, playback: &controls }, 1.3);
        assert_eq!(bar_of(&w, 0x4203), Some(mtouch::bar_level(3, BarColour::Green, false)));
        // disabled: nothing is written
        let mut off = Lights::new(Config { enabled: false, min_interval: 0.0 });
        assert!(off.render(&Scene { ctx: Some(&c1), strips: None, playback: &controls }, 1.0).is_empty());
    }

    #[test]
    fn a_touched_strip_and_a_picked_up_fader_show_the_echo_and_hand_back_at_the_lift() {
        let mut l = Lights::new(fast());
        let c = ctx(json!([{"n":1,"kind":"attribute","name":"ColorRGB_R","avail":"available","val":"value","abs":40.0,"vc":1}]),
                    json!([{"n":201,"tgt":1,"cls":"Sequence","name":"A","fd":"Master","lvl":30,"act":1,"col":"00ff00"}]));
        let pbs = vec![PlaybackControl { echo: Some(0.9), ..pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 201, true, false) }];
        let strips = ParamStrips { present: true, base_slot: 1, touching: [true, false, false, false], echo: [Some(0.8), None, None, None] };
        let w = l.render(&Scene { ctx: Some(&c), strips: Some(strips), playback: &pbs }, 0.0);
        assert_eq!(bar_of(&w, param_bar_id(1)), Some(mtouch::bar_level(8, BarColour::Blue, false)), "the strip shows its echo (80 %), not the read-back 40 %");
        assert_eq!(bar_of(&w, 0x4203), Some(mtouch::bar_level(9, BarColour::Green, false)), "the fader shows its position (90 %), not the read-back 30 %");
        // The lift: no echo, the read-back again.
        let pbs = vec![pb(Model::MTouch, 0x4203, Light::BarRgb, "PB1", 201, false, false)];
        let strips = ParamStrips { present: true, base_slot: 1, touching: [false; 4], echo: [None; 4] };
        let w = l.render(&Scene { ctx: Some(&c), strips: Some(strips), playback: &pbs }, 0.1);
        assert_eq!(bar_of(&w, param_bar_id(1)), Some(mtouch::bar_level(4, BarColour::Blue, false)));
        assert_eq!(bar_of(&w, 0x4203), Some(mtouch::bar_level(3, BarColour::Green, false)));
        // A mixed selection never shows a single echoed level.
        let m = ctx(json!([{"n":1,"kind":"attribute","name":"ColorRGB_R","avail":"mixed","val":"mixed","abs":40.0,"vc":1}]), json!([]));
        let strips = ParamStrips { present: true, base_slot: 1, touching: [true, false, false, false], echo: [Some(0.8), None, None, None] };
        let w = l.render(&Scene { ctx: Some(&m), strips: Some(strips), playback: &[] }, 0.2);
        assert_eq!(bar_of(&w, param_bar_id(1)), Some(mixed_bar(BarColour::Blue)));
    }

    #[test]
    fn a_rebind_in_flight_is_dark_for_the_grace_then_unavailable() {
        let mut l = Lights::new(fast());
        let c = ctx(json!([]), json!([{"n":201,"tgt":1,"cls":"Sequence","name":"A","fd":"Master","lvl":30,"act":0}]));
        let pbs = vec![pb(Model::MTouch, 0x4205, Light::KeyRedGreen, "PB1", 201, false, false)];
        let w = l.render(&Scene { ctx: Some(&c), strips: None, playback: &pbs }, 0.0);
        assert_eq!(key_of(&w, 0x4205), Some(key_red_green(PlaybackState::Assigned).value()));
        l.set_rebinding(true);
        let w = l.render(&Scene { ctx: None, strips: None, playback: &pbs }, 0.1);
        assert_eq!(key_of(&w, 0x4205), Some(0), "a bank change in flight is dark, not the stale pattern");
        assert_eq!(l.stats.stale_renders, 0);
        let w = l.render(&Scene { ctx: None, strips: None, playback: &pbs }, 0.1 + REBIND_GRACE + 0.01);
        assert_eq!(key_of(&w, 0x4205), Some(key_red_green(PlaybackState::Unavailable).value()), "past the grace it is staleness");
        // Plain staleness (no rebind) is the pattern at once.
        let mut s = Lights::new(fast());
        s.render(&Scene { ctx: Some(&c), strips: None, playback: &pbs }, 0.0);
        let w = s.render(&Scene { ctx: None, strips: None, playback: &pbs }, 0.1);
        assert_eq!(key_of(&w, 0x4205), Some(key_red_green(PlaybackState::Unavailable).value()));
    }
}
