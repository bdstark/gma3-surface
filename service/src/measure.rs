//! KB-24: the measurement harness (`mtpnxk measure`). A realistic simultaneous workload is played through the
//! simulated surfaces and the real link against the real plugin: NX-K key taps and a feedback probe (a key whose
//! press must change a console state item), two rotaries (2 and 3) turning, an M-Touch parameter strip (slot 1) dragged, a playback
//! fader moved and a playback button tapped, all at once, for a number of seconds. The report publishes:
//!
//! - **input latency**: ack round trips of key events and of ctl boundaries (touches, buttons), and how long
//!   queued motion waited in the service before it was sent (motion is never acknowledged, so its round trip is
//!   not observable; the plugin's `bench` press-to-effect covers the console end for key taps);
//! - **feedback latency**: probe key press to the first state message in which the probed item changed;
//! - **missed events**: lost (retransmits exhausted), superseded (a press unacknowledged at its release), aged,
//!   stale and overflowed motion, loss the plugin reported, refusals by code;
//! - **ordering**: `out-of-order` refusals (the plugin saw a later per-device sequence first) and superseded
//!   presses, both of which must stay at zero;
//! - **plugin loop**: the heartbeat's load report (plugin 0.11.0): the longest tick, the most datagrams one tick
//!   took and the ticks that exhausted the per-tick budget.
//!
//! Every figure is judged against the KB-24 thresholds below (loopback host, see docs/qualification-matrix.md);
//! a metric without samples is "n.a.", never a pass. The thresholds are proposed values until the operator
//! agrees them; a run against a different host or network publishes its own.

use crate::banks;
use crate::link;
use crate::strips;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The simulated workload, per second of run.
pub struct Workload {
    pub key_rate: f64,
    pub motion_rate: f64,
    pub probe_rate: f64,
    pub probe_key: &'static str,
    pub strip: bool,
    pub playback: bool,
}

/// The script's content spans this many milliseconds; the simulator repeats it after a 200 ms gap, so one
/// repetition is exactly five seconds and the per-second rates hold.
const CONTENT_MS: u64 = 4800;
const PERIOD_MS: u64 = 5000;

/// Builds the sim script (see sim.rs for the grammar) of one five-second repetition.
pub fn workload(w: &Workload) -> String {
    let mut steps: Vec<(u64, String)> = Vec::new();
    let spread = |rate: f64, offset: u64| -> Vec<u64> {
        let n = (rate * PERIOD_MS as f64 / 1000.0).round().max(0.0) as u64;
        (0..n).map(|i| offset + i * CONTENT_MS / n.max(1)).filter(|t| *t < CONTENT_MS).collect()
    };
    for t in spread(w.key_rate, 10) {
        steps.push((t, "5:tap".into()));
    }
    for t in spread(w.probe_rate, 190) {
        // Record leaves "Store" on the command line: a Clear 150 ms before it empties the line first (digits typed
        // by the key taps would otherwise precede the keyword and the plugin's `pending` reader would not see Store),
        // and a Clear 400 ms after it removes it (long enough for the console's state read-back, about 170 ms on
        // onPC). Clear also clears the selection and programmer, which refuses the motion afterwards. A toggling
        // probe (HighLight) needs neither and keeps the selection for the motion part.
        if w.probe_key == "Record" {
            steps.push((t - 150, "Clear:tap".into()));
        }
        steps.push((t, format!("{}:tap", w.probe_key)));
        if w.probe_key == "Record" {
            steps.push((t + 400, "Clear:tap".into()));
        }
    }
    // Rotaries 2 and 3: slot 1 is the strip's (a touched slot refuses another control's motion as `conflict`).
    for t in spread(w.motion_rate, 0) {
        steps.push((t, "rot2:+1".into()));
        steps.push((t + 3, "rot3:-1".into()));
    }
    if w.strip {
        steps.push((5, "strip1:t64".into()));
        let mut t = 30;
        let mut i = 0u32;
        while t < CONTENT_MS - 20 {
            // A slow triangle between 64 and 160 (one detent or so per report at 25 ms).
            let phase = (i % 96) as i32;
            let v = if phase < 48 { 64 + phase * 2 } else { 160 - (phase - 48) * 2 };
            steps.push((t, format!("strip1:m{v}")));
            t += 25;
            i += 1;
        }
        steps.push((CONTENT_MS - 10, "strip1:lift".into()));
    }
    if w.playback {
        steps.push((15, "pb1:t40".into()));
        let mut t = 65;
        let mut i = 0u32;
        while t < CONTENT_MS - 40 {
            let phase = (i % 40) as i32;
            let v = if phase < 20 { 40 + phase * 4 } else { 120 - (phase - 20) * 4 };
            steps.push((t, format!("pb1:m{v}")));
            t += 50;
            i += 1;
        }
        steps.push((CONTENT_MS - 30, "pb1:lift".into()));
        for k in 0..(PERIOD_MS / 1000) {
            steps.push((500 + k * 1000, "pfa1:tap".into()));
        }
    }
    steps.sort_by_key(|(t, _)| *t);
    let mut out = Vec::with_capacity(steps.len());
    let mut last = 0;
    for (t, s) in steps {
        out.push(format!("{s}@{}", t - last));
        last = t;
    }
    out.join(",")
}

/// Counters and sample counts at the end of the warm-up: the report covers only what came after.
pub struct Snapshot {
    stats: link::Stats,
    strips: strips::Stats,
    banks: banks::Stats,
    rtt_key: usize,
    rtt_ctl: usize,
    motion_queue: usize,
    feedback: usize,
    feedback_missed: u64,
    effect: usize,
    load: usize,
    refusal_codes: BTreeMap<String, u64>,
    hellos: u64,
}

impl Snapshot {
    pub fn of(link: &link::Link, strips: &strips::Strips, banks: &banks::Banks) -> Snapshot {
        Snapshot {
            stats: link.stats.clone(),
            strips: strips.stats.clone(),
            banks: banks.stats.clone(),
            rtt_key: link.rtt_key_ms.len(),
            rtt_ctl: link.rtt_ctl_ms.len(),
            motion_queue: link.motion_queue_ms.len(),
            feedback: link.feedback_ms.len(),
            feedback_missed: link.feedback_missed,
            effect: link.effect_ms.len(),
            load: link.plugin_load.len(),
            refusal_codes: link.refusal_codes.clone(),
            hellos: link.stats.hellos,
        }
    }
}

pub struct Setup {
    pub seconds: f64,
    pub key_rate: f64,
    pub motion_rate: f64,
    pub probe_rate: f64,
    pub probe_key: String,
    pub probe_item: String,
    pub strip: bool,
    pub playback: bool,
    pub plugin: String,
    pub paired: bool,
    pub context_at_start: bool,
    pub warmup_s: f64,
}

/// Percentiles of one sample set, in milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub median: f64,
    pub p95: f64,
    pub p99: f64,
    pub worst: f64,
}

pub fn summarise(samples: &[f64]) -> Option<Summary> {
    if samples.is_empty() {
        return None;
    }
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p = |q: f64| s[((s.len() - 1) as f64 * q).round() as usize];
    Some(Summary { n: s.len(), median: p(0.5), p95: p(0.95), p99: p(0.99), worst: s[s.len() - 1] })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
    /// No samples (the plugin was not started with `bench`, the console's state did not move, an older
    /// plugin without the load report): nothing is claimed.
    NotAvailable,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub value: Option<f64>,
    pub threshold: String,
    pub verdict: Verdict,
}

/// The KB-24 thresholds for a loopback host (service and onPC on one machine). Proposed; see the matrix.
pub mod thresholds {
    // Agreed 2026-10-11 from the onPC runs (plugin tick 13 ms idle, 34 ms with keys; key acks 40-46 ms median,
    // 57-70 ms p95): measured plus margin. The zero-loss checks are unchanged.
    pub const KEY_ACK_P95_MS: f64 = 80.0;
    pub const KEY_ACK_WORST_MS: f64 = 300.0;
    pub const CTL_ACK_P95_MS: f64 = 80.0;
    pub const MOTION_QUEUE_P95_MS: f64 = 15.0; // agreed 2026-10-11: the 25 ms per-control interval makes 10-12 ms p95 the norm
    pub const MOTION_QUEUE_WORST_MS: f64 = 50.0;
    pub const FEEDBACK_P95_MS: f64 = 300.0;
    pub const EFFECT_P95_MS: f64 = 100.0;
    pub const RETRANSMIT_RATIO: f64 = 0.01;
    pub const FEEDBACK_MISSED_RATIO: f64 = 0.10;
    pub const PLUGIN_TICK_MAX_MS: f64 = 200.0;
}

pub struct Report {
    pub setup: Vec<(String, String)>,
    pub metrics: Vec<(String, Option<Summary>)>,
    pub counters: BTreeMap<String, u64>,
    pub refusals: BTreeMap<String, u64>,
    pub load: Option<(f64, f64, u64, u64, u64)>,
    /// Where the plugin's tick time went over the run (ms): module servicing, datagrams, state/context.
    pub load_phases: Option<(f64, f64, f64)>,
    /// Scheduling diagnostics over the run: the most distinct coroutines in one window, overlapping ticks.
    pub load_sched: Option<(u64, u64)>,
    pub checks: Vec<Check>,
}

fn delta(a: u64, b: u64) -> u64 {
    b.saturating_sub(a)
}

pub fn report(link: &link::Link, strips: &strips::Strips, banks: &banks::Banks, before: &Snapshot, setup: Setup) -> Report {
    let st = &link.stats;
    let b = &before.stats;
    let mut counters = BTreeMap::new();
    let mut c = |k: &str, v: u64| {
        counters.insert(k.to_string(), v);
    };
    c("events", delta(b.events, st.events));
    c("acked", delta(b.acked, st.acked));
    c("refused", delta(b.refused, st.refused));
    c("retransmitted", delta(b.retransmitted, st.retransmitted));
    c("lost", delta(b.lost, st.lost));
    c("superseded", delta(b.superseded, st.superseded));
    c("rejected", delta(b.rejected, st.rejected));
    c("old_seq", delta(b.old_seq, st.old_seq));
    c("link_downs", delta(b.link_downs, st.link_downs));
    c("states", delta(b.states, st.states));
    c("deltas_ignored", delta(b.deltas_ignored, st.deltas_ignored));
    c("contexts", delta(b.contexts, st.contexts));
    c("ctl_events", delta(b.ctl_events, st.ctl_events));
    c("ctl_sent", delta(b.ctl_sent, st.ctl_sent));
    c("ctl_coalesced", delta(b.ctl_coalesced, st.ctl_coalesced));
    c("ctl_unbound", delta(b.ctl_unbound, st.ctl_unbound));
    c("ctl_aged", delta(b.ctl_aged, st.ctl_aged));
    c("ctl_stale", delta(b.ctl_stale, st.ctl_stale));
    c("ctl_refused", delta(b.ctl_refused, st.ctl_refused));
    c("ctl_lost_reported", delta(b.ctl_lost_reported, st.ctl_lost_reported));
    c("ctl_superseded", delta(b.ctl_superseded, st.ctl_superseded));
    c("ctl_overflow", delta(b.ctl_overflow, st.ctl_overflow));
    c("strip_reports", delta(before.strips.reports, strips.stats.reports));
    c("strip_detents", delta(before.strips.detents, strips.stats.detents));
    c("strip_rebound_dropped", delta(before.strips.rebound_dropped, strips.stats.rebound_dropped));
    c("bank_downs", delta(before.banks.downs, banks.stats.downs));
    c("bank_ups", delta(before.banks.ups, banks.stats.ups));
    c("bank_positions", delta(before.banks.positions, banks.stats.positions));
    c("bank_waiting_pickup", delta(before.banks.waiting_pickup, banks.stats.waiting_pickup));
    c("feedback_missed", delta(before.feedback_missed, link.feedback_missed) + u64::from(link.feedback_probe_age(f64::MAX).is_some()));
    let no_effect = link.effect_ms[before.effect.min(link.effect_ms.len())..].iter().filter(|m| **m < 0).count() as u64;
    c("effect_none", no_effect);

    // Refusal codes over the run: the link's cumulative per-code counters minus the warm-up snapshot (the bounded
    // refusal log stays for diagnostics; review of PR #9).
    let mut refusals: BTreeMap<String, u64> = BTreeMap::new();
    for (code, n) in &link.refusal_codes {
        let d = delta(before.refusal_codes.get(code).copied().unwrap_or(0), *n);
        if d > 0 {
            refusals.insert(code.clone(), d);
        }
    }
    c("hellos", delta(before.hellos, st.hellos));

    let slice = |v: &[f64], from: usize| -> Vec<f64> { v[from.min(v.len())..].to_vec() };
    let key_ack = summarise(&slice(&link.rtt_key_ms, before.rtt_key));
    let ctl_ack = summarise(&slice(&link.rtt_ctl_ms, before.rtt_ctl));
    let motion_queue = summarise(&slice(&link.motion_queue_ms, before.motion_queue));
    let feedback = summarise(&slice(&link.feedback_ms, before.feedback));
    let effects: Vec<f64> = link.effect_ms[before.effect.min(link.effect_ms.len())..].iter().filter(|m| **m >= 0).map(|m| *m as f64).collect();
    let effect = summarise(&effects);

    let loads = &link.plugin_load[before.load.min(link.plugin_load.len())..];
    let load = if loads.is_empty() {
        None
    } else {
        let tmax = loads.iter().map(|l| l.tmax).fold(0.0, f64::max);
        let n: u64 = loads.iter().map(|l| l.n).sum();
        let tavg = if n > 0 { loads.iter().map(|l| l.tavg * l.n as f64).sum::<f64>() / n as f64 } else { 0.0 };
        let pmax = loads.iter().map(|l| l.pmax).max().unwrap_or(0);
        let full: u64 = loads.iter().map(|l| l.full).sum();
        Some((tmax, tavg, pmax, full, n))
    };
    let load_sched = if loads.is_empty() { None } else { Some((loads.iter().map(|l| l.co).max().unwrap_or(0), loads.iter().map(|l| l.ovl).sum())) };
    let load_phases = if loads.is_empty() { None } else { Some((loads.iter().map(|l| l.smod).sum(), loads.iter().map(|l| l.spkt).sum(), loads.iter().map(|l| l.sstate).sum())) };

    let mut checks = Vec::new();
    let mut lat = |name: &str, s: &Option<Summary>, pick: fn(&Summary) -> f64, limit: f64| {
        let (value, verdict) = match s {
            Some(s) => {
                let v = pick(s);
                (Some(v), if v <= limit { Verdict::Pass } else { Verdict::Fail })
            }
            None => (None, Verdict::NotAvailable),
        };
        checks.push(Check { name: name.into(), value, threshold: format!("<= {limit:.0} ms"), verdict });
    };
    lat("key ack p95", &key_ack, |s| s.p95, thresholds::KEY_ACK_P95_MS);
    lat("key ack worst", &key_ack, |s| s.worst, thresholds::KEY_ACK_WORST_MS);
    lat("ctl boundary ack p95", &ctl_ack, |s| s.p95, thresholds::CTL_ACK_P95_MS);
    lat("motion queue p95", &motion_queue, |s| s.p95, thresholds::MOTION_QUEUE_P95_MS);
    lat("motion queue worst", &motion_queue, |s| s.worst, thresholds::MOTION_QUEUE_WORST_MS);
    lat("feedback p95", &feedback, |s| s.p95, thresholds::FEEDBACK_P95_MS);
    lat("press-to-effect p95", &effect, |s| s.p95, thresholds::EFFECT_P95_MS);
    let mut zero = |name: &str, key: &str| {
        let v = counters.get(key).copied().unwrap_or(0);
        checks.push(Check { name: name.into(), value: Some(v as f64), threshold: "= 0".into(), verdict: if v == 0 { Verdict::Pass } else { Verdict::Fail } });
    };
    zero("lost events", "lost");
    zero("superseded presses", "superseded");
    zero("superseded ctl downs", "ctl_superseded");
    zero("plugin-reported ctl loss", "ctl_lost_reported");
    zero("aged motion", "ctl_aged");
    zero("stale motion", "ctl_stale");
    zero("overflowed positions", "ctl_overflow");
    zero("rejected plugin packets", "rejected");
    zero("link downs", "link_downs");
    zero("deltas ignored", "deltas_ignored");
    zero("re-pairings during the run", "hellos");
    let ooo = refusals.get("out-of-order").copied().unwrap_or(0);
    checks.push(Check { name: "out-of-order refusals".into(), value: Some(ooo as f64), threshold: "= 0".into(), verdict: if ooo == 0 { Verdict::Pass } else { Verdict::Fail } });
    let events = counters.get("events").copied().unwrap_or(0);
    let retx = counters.get("retransmitted").copied().unwrap_or(0);
    let ratio = if events > 0 { retx as f64 / events as f64 } else { 0.0 };
    checks.push(Check { name: "retransmit ratio".into(), value: Some(ratio), threshold: format!("<= {:.0}%", thresholds::RETRANSMIT_RATIO * 100.0), verdict: if ratio <= thresholds::RETRANSMIT_RATIO { Verdict::Pass } else { Verdict::Fail } });
    if setup.probe_rate > 0.0 {
        // Agreed 2026-10-11: a probe is "missed" when the keyword was not read back before its Clear 400 ms later; on
        // onPC 2.5.1 one in ten still is (the console's own frame and read timing), so the line is a tenth of the probes.
        let missed = counters.get("feedback_missed").copied().unwrap_or(0);
        let total = missed + feedback.as_ref().map(|s| s.n as u64).unwrap_or(0);
        let ratio = if total > 0 { missed as f64 / total as f64 } else { 0.0 };
        let verdict = if feedback.is_none() { Verdict::NotAvailable } else if ratio <= thresholds::FEEDBACK_MISSED_RATIO { Verdict::Pass } else { Verdict::Fail };
        checks.push(Check { name: "feedback probes missed".into(), value: Some(ratio), threshold: format!("<= {:.0}% of probes", thresholds::FEEDBACK_MISSED_RATIO * 100.0), verdict });
    }
    match &load {
        Some((tmax, _, pmax, full, _)) => {
            checks.push(Check { name: "plugin tick max".into(), value: Some(*tmax), threshold: format!("<= {:.0} ms", thresholds::PLUGIN_TICK_MAX_MS), verdict: if *tmax <= thresholds::PLUGIN_TICK_MAX_MS { Verdict::Pass } else { Verdict::Fail } });
            checks.push(Check { name: "plugin ticks at the packet budget".into(), value: Some(*full as f64), threshold: "= 0".into(), verdict: if *full == 0 { Verdict::Pass } else { Verdict::Fail } });
            checks.push(Check { name: "plugin datagrams per tick max".into(), value: Some(*pmax as f64), threshold: "< 32".into(), verdict: if *pmax < 32 { Verdict::Pass } else { Verdict::Fail } });
        }
        None => {
            checks.push(Check { name: "plugin tick max".into(), value: None, threshold: format!("<= {:.0} ms", thresholds::PLUGIN_TICK_MAX_MS), verdict: Verdict::NotAvailable });
        }
    }

    let setup_rows = vec![
        ("plugin".to_string(), setup.plugin.clone()),
        ("paired".into(), setup.paired.to_string()),
        ("context known at start".into(), setup.context_at_start.to_string()),
        ("warm-up".into(), format!("{:.2} s", setup.warmup_s)),
        ("workload".into(), format!("{:.0} s: keys {}/s, probe {} ({}) {}/s, rotaries 2+3 at {}/s each, strip {}, playback {}", setup.seconds, setup.key_rate, setup.probe_key, setup.probe_item, setup.probe_rate, setup.motion_rate, if setup.strip { "on" } else { "off" }, if setup.playback { "on" } else { "off" })),
    ];
    Report {
        setup: setup_rows,
        metrics: vec![
            ("key ack round trip".into(), key_ack),
            ("ctl boundary ack round trip".into(), ctl_ack),
            ("motion queue delay".into(), motion_queue),
            (format!("feedback ({} -> {})", setup.probe_key, setup.probe_item), feedback),
            ("press-to-effect (plugin bench)".into(), effect),
        ],
        counters,
        refusals,
        load,
        load_phases,
        load_sched,
        checks,
    }
}

impl Report {
    pub fn failed(&self) -> bool {
        self.checks.iter().any(|c| c.verdict == Verdict::Fail)
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        out.push_str("measure: KB-24 workload report\n");
        for (k, v) in &self.setup {
            out.push_str(&format!("  {k}: {v}\n"));
        }
        for (name, s) in &self.metrics {
            match s {
                Some(s) => out.push_str(&format!("{name}: n={} median={:.1} p95={:.1} p99={:.1} worst={:.1} ms\n", s.n, s.median, s.p95, s.p99, s.worst)),
                None => out.push_str(&format!("{name}: no samples\n")),
            }
        }
        match &self.load {
            Some((tmax, tavg, pmax, full, n)) => {
                out.push_str(&format!("plugin loop: ticks={n} tick max={tmax:.2} ms mean={tavg:.3} ms datagrams/tick max={pmax} ticks at budget={full}\n"));
                if let Some((co, ovl)) = self.load_sched {
                    out.push_str(&format!("plugin loop scheduling: coroutines per window max={co} overlapping ticks={ovl}\n"));
                }
                if let Some((m, p, s)) = self.load_phases {
                    let total = (m + p + s).max(0.001);
                    out.push_str(&format!("plugin loop phases: modules {:.0}% ({m:.0} ms) datagrams {:.0}% ({p:.0} ms) state/context {:.0}% ({s:.0} ms)\n", m / total * 100.0, p / total * 100.0, s / total * 100.0));
                }
            }
            None => out.push_str("plugin loop: no load report (plugin older than 0.11.0, or no heartbeat reply)\n"),
        }
        let counters: Vec<String> = self.counters.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push_str(&format!("counters: {}\n", counters.join(" ")));
        if !self.refusals.is_empty() {
            let r: Vec<String> = self.refusals.iter().map(|(k, v)| format!("{k}={v}")).collect();
            out.push_str(&format!("refusals (run, by code): {}\n", r.join(" ")));
        }
        for c in &self.checks {
            let v = match c.value {
                Some(v) if c.name == "retransmit ratio" || c.name == "feedback probes missed" => format!("{:.1}%", v * 100.0),
                Some(v) => format!("{v:.1}"),
                None => "n.a.".into(),
            };
            out.push_str(&format!("check {:<4} {}: {} ({})\n", match c.verdict { Verdict::Pass => "PASS", Verdict::Fail => "FAIL", Verdict::NotAvailable => "n.a." }, c.name, v, c.threshold));
        }
        out.push_str(&format!("verdict: {}\n", if self.failed() { "FAIL" } else { "PASS (n.a. checks claim nothing)" }));
        out
    }

    pub fn json(&self) -> Value {
        let s = |s: &Option<Summary>| match s {
            Some(s) => json!({ "n": s.n, "median_ms": s.median, "p95_ms": s.p95, "p99_ms": s.p99, "worst_ms": s.worst }),
            None => Value::Null,
        };
        json!({
            "setup": self.setup.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<serde_json::Map<_, _>>(),
            "metrics": self.metrics.iter().map(|(k, v)| (k.clone(), s(v))).collect::<serde_json::Map<_, _>>(),
            "plugin_loop": self.load.map(|(tmax, tavg, pmax, full, n)| json!({ "ticks": n, "tick_max_ms": tmax, "tick_mean_ms": tavg, "datagrams_per_tick_max": pmax, "ticks_at_budget": full, "phases_ms": self.load_phases.map(|(m, p, s)| json!({ "modules": m, "datagrams": p, "state_context": s })), "scheduling": self.load_sched.map(|(co, ovl)| json!({ "coroutines_max": co, "overlapping_ticks": ovl })) })),
            "counters": self.counters,
            "refusals_by_code": self.refusals,
            "checks": self.checks.iter().map(|c| json!({ "name": c.name, "value": c.value, "threshold": c.threshold, "verdict": match c.verdict { Verdict::Pass => "pass", Verdict::Fail => "fail", Verdict::NotAvailable => "n.a." } })).collect::<Vec<_>>(),
            "verdict": if self.failed() { "fail" } else { "pass" },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workload_script_parses_holds_its_rates_and_spans_one_five_second_repetition() {
        let w = Workload { key_rate: 4.0, motion_rate: 50.0, probe_rate: 2.0, probe_key: "Record", strip: true, playback: true };
        let script = workload(&w);
        let steps = crate::sim::parse_script(&script).expect("the generated script parses");
        let last = steps.last().unwrap().at_ms;
        // The last step is the strip lift (CONTENT_MS - 10) or the release of a tap started just before it.
        assert!(last >= CONTENT_MS - 40 && last < CONTENT_MS + 200, "content ends around {CONTENT_MS} ms: {last}");
        let count = |needle: &str| script.split(',').filter(|s| s.starts_with(needle)).count();
        assert_eq!(count("5:tap"), 20, "4 taps/s over 5 s");
        assert_eq!(count("Record:tap"), 10);
        assert_eq!(count("Clear:tap"), 20, "each probe is preceded and followed by a Clear");
        assert_eq!(count("rot2:+1"), 250, "50 detents/s over 5 s");
        assert_eq!(count("rot3:-1"), 250);
        assert_eq!(count("strip1:t"), 1);
        assert_eq!(count("strip1:lift"), 1);
        assert_eq!(count("pb1:t"), 1);
        assert_eq!(count("pb1:lift"), 1);
        assert_eq!(count("pfa1:tap"), 5);
        // Every delay is non-negative and the steps are in time order.
        let mut prev = 0;
        for s in &steps {
            assert!(s.at_ms >= prev);
            prev = s.at_ms;
        }
    }

    #[test]
    fn the_workload_leaves_out_what_was_switched_off() {
        let w = Workload { key_rate: 1.0, motion_rate: 10.0, probe_rate: 0.0, probe_key: "Record", strip: false, playback: false };
        let script = workload(&w);
        assert!(!script.contains("strip1") && !script.contains("pb1") && !script.contains("pfa1") && !script.contains("Record"));
        assert_eq!(script.split(',').filter(|s| s.starts_with("rot2:")).count(), 50);
        let w = Workload { key_rate: 0.0, motion_rate: 0.0, probe_rate: 2.0, probe_key: "HighLight", strip: false, playback: false };
        let script = workload(&w);
        assert_eq!(script.split(',').filter(|s| s.starts_with("HighLight:tap")).count(), 10);
        assert!(!script.contains("Clear"), "a toggling probe is not followed by Clear");
    }

    #[test]
    fn percentiles_follow_the_bench_definition() {
        let s = summarise(&[5.0, 1.0, 3.0, 2.0, 4.0]).unwrap();
        assert_eq!((s.n, s.median, s.worst), (5, 3.0, 5.0));
        assert_eq!(s.p95, 5.0);
        assert!(summarise(&[]).is_none());
    }
}
