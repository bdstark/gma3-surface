//! KB-20: the `run` surface: an NX-K keypad and an M-Touch, each optional and each reopened on its own
//! (review of PR #5: an M-Touch-only installation must poll its strips, and losing the NX-K must not
//! interrupt the M-Touch). A device absent at start is retried every two seconds; a device whose reads fail
//! is dropped and reopened the same way; the loop keeps running with whichever device is present. Losing the
//! M-Touch ends every touched strip (the loop turns the `Err` of `poll_strips` into releases); losing the
//! keypad is reported once (`disconnected`) so the loop releases the keys, rotary pushes and the Bank modifier
//! it held, and a reopened keypad is announced (`reconnected`) so its LEDs are repainted.
//!
//! Reports an M-Touch queued before the first poll are delivered at open (KB-16 record): the first 250 ms
//! after an open are discarded, never used to seed a touch or a level.

use crate::mtouch::{self, Event as MtEvent};
use crate::nxk::usb::Surface;
use crate::nxk::Event;
use anyhow::Result;
use std::time::{Duration, Instant};

/// A raw M-Touch or M-Play packet source (the USB device, or a fake in tests).
pub trait StripDevice {
    fn poll(&mut self) -> Result<Vec<Vec<u8>>>;
    fn name(&self) -> String;
    /// KB-21: one output write (page display, LED, bar); nothing by default.
    fn send(&mut self, _out: mtouch::Output) {}
}

impl StripDevice for mtouch::usb::UsbSurface {
    fn poll(&mut self) -> Result<Vec<Vec<u8>>> {
        mtouch::usb::UsbSurface::poll(self)
    }
    fn name(&self) -> String {
        mtouch::usb::UsbSurface::name(self).to_string()
    }
    fn send(&mut self, out: mtouch::Output) {
        mtouch::usb::UsbSurface::send(self, out)
    }
}

const RETRY: Duration = Duration::from_secs(2);
const DISCARD_AFTER_OPEN: Duration = Duration::from_millis(250);

pub struct Combined<K: Surface, M: StripDevice> {
    open_keypad: Box<dyn FnMut() -> Result<K>>,
    open_mtouch: Option<Box<dyn FnMut() -> Result<M>>>,
    keypad: Option<K>,
    mtouch: Option<M>,
    next_keypad: Instant,
    next_mtouch: Instant,
    discard_until: Instant,
    keypad_reconnected: bool,
    keypad_lost: bool,
    announced_absent: bool,
    /// KB-21: the M-Play, opened and reopened like the M-Touch.
    open_mplay: Option<Box<dyn FnMut() -> Result<M>>>,
    mplay: Option<M>,
    next_mplay: Instant,
    discard_mplay_until: Instant,
}

impl<K: Surface, M: StripDevice> Combined<K, M> {
    /// `open_mtouch` None: the M-Touch is not wanted (`--mtouch off` or `--strips off`).
    pub fn new(open_keypad: Box<dyn FnMut() -> Result<K>>, open_mtouch: Option<Box<dyn FnMut() -> Result<M>>>) -> Self {
        let now = Instant::now();
        Combined { open_keypad, open_mtouch, keypad: None, mtouch: None, next_keypad: now, next_mtouch: now, discard_until: now, keypad_reconnected: false, keypad_lost: false, announced_absent: false,
                   open_mplay: None, mplay: None, next_mplay: now, discard_mplay_until: now }
    }

    /// KB-21: also open an M-Play (`--mplay auto`), retried while absent like the other devices.
    pub fn with_mplay(mut self, open_mplay: Option<Box<dyn FnMut() -> Result<M>>>) -> Self {
        self.open_mplay = open_mplay;
        self
    }

    fn try_open_mplay(&mut self) {
        if self.mplay.is_some() || Instant::now() < self.next_mplay {
            return;
        }
        self.next_mplay = Instant::now() + RETRY;
        let Some(open) = self.open_mplay.as_mut() else { return };
        match open() {
            Ok(m) => {
                eprintln!("mplay: {} opened; its playback buttons and bank keys are live (faders: KB-22)", m.name());
                self.discard_mplay_until = Instant::now() + DISCARD_AFTER_OPEN;
                self.mplay = Some(m);
            }
            Err(e) => {
                if self.mplay.is_none() && !self.announced_absent {
                    eprintln!("mplay: not open ({e:#}); retrying every {} s", RETRY.as_secs());
                }
            }
        }
    }

    fn try_open_keypad(&mut self) {
        if self.keypad.is_some() || Instant::now() < self.next_keypad {
            return;
        }
        self.next_keypad = Instant::now() + RETRY;
        match (self.open_keypad)() {
            Ok(k) => {
                eprintln!("nxk: {} open", k.name());
                self.keypad = Some(k);
                self.keypad_reconnected = true;
                self.announced_absent = false;
            }
            Err(e) => {
                if !self.announced_absent {
                    eprintln!("nxk: {e:#}; retrying every {} s (the M-Touch, if any, keeps working)", RETRY.as_secs());
                    self.announced_absent = true;
                }
            }
        }
    }

    fn try_open_mtouch(&mut self) {
        let Some(open) = self.open_mtouch.as_mut() else { return };
        if self.mtouch.is_some() || Instant::now() < self.next_mtouch {
            return;
        }
        self.next_mtouch = Instant::now() + RETRY;
        if let Ok(dev) = open() {
            eprintln!("strips: {} open; its four base-channel strips are parameter strips", dev.name());
            self.mtouch = Some(dev);
            self.discard_until = Instant::now() + DISCARD_AFTER_OPEN;
        }
    }

    #[cfg(test)]
    pub fn has_keypad(&self) -> bool {
        self.keypad.is_some()
    }
    #[cfg(test)]
    pub fn has_mtouch(&self) -> bool {
        self.mtouch.is_some()
    }
}

impl<K: Surface, M: StripDevice> Surface for Combined<K, M> {
    /// Keypad events, or none while the keypad is absent. Never `Err`: a failing keypad is dropped and reopened.
    fn poll(&mut self) -> Result<Vec<Event>> {
        self.try_open_keypad();
        let Some(k) = self.keypad.as_mut() else { return Ok(Vec::new()) };
        match k.poll() {
            Ok(ev) => Ok(ev),
            Err(e) => {
                eprintln!("nxk: {e:#}; reopening in {} s; its held keys are released", RETRY.as_secs());
                self.keypad = None;
                self.keypad_lost = true;
                self.next_keypad = Instant::now() + RETRY;
                Ok(Vec::new())
            }
        }
    }

    fn led(&mut self, id: u16, value: u16) {
        if let Some(k) = self.keypad.as_mut() {
            k.led(id, value);
        }
    }

    fn name(&self) -> String {
        match (&self.keypad, &self.mtouch) {
            (Some(k), Some(m)) => format!("{} + {}", k.name(), m.name()),
            (Some(k), None) => k.name(),
            (None, Some(m)) => format!("no NX-K + {}", m.name()),
            (None, None) => "no NX-K, no M-Touch (retrying)".into(),
        }
    }

    fn reconnected(&mut self) -> bool {
        std::mem::take(&mut self.keypad_reconnected)
    }

    fn disconnected(&mut self) -> bool {
        std::mem::take(&mut self.keypad_lost)
    }

    fn poll_mplay(&mut self) -> Result<Vec<MtEvent>> {
        self.try_open_mplay();
        let Some(dev) = self.mplay.as_mut() else { return Ok(Vec::new()) };
        let packets = match dev.poll() {
            Ok(p) => p,
            Err(e) => {
                self.mplay = None;
                self.next_mplay = Instant::now() + RETRY;
                return Err(e);
            }
        };
        if Instant::now() < self.discard_mplay_until {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for p in packets {
            if p.is_empty() {
                continue;
            }
            for r in mtouch::decode(&p) {
                out.extend(mtouch::events(mtouch::Model::MPlay, &r));
            }
        }
        Ok(out)
    }

    fn device_present(&self, model: mtouch::Model) -> bool {
        match model {
            mtouch::Model::MTouch => self.mtouch.is_some(),
            mtouch::Model::MPlay => self.mplay.is_some(),
        }
    }

    fn output(&mut self, model: mtouch::Model, out: mtouch::Output) {
        match model {
            mtouch::Model::MTouch => { if let Some(d) = self.mtouch.as_mut() { d.send(out); } }
            mtouch::Model::MPlay => { if let Some(d) = self.mplay.as_mut() { d.send(out); } }
        }
    }

    fn poll_strips(&mut self) -> Result<Vec<MtEvent>> {
        self.try_open_mtouch();
        let Some(dev) = self.mtouch.as_mut() else { return Ok(Vec::new()) };
        let packets = match dev.poll() {
            Ok(p) => p,
            Err(e) => {
                self.mtouch = None;
                self.next_mtouch = Instant::now() + RETRY;
                return Err(e);
            }
        };
        if Instant::now() < self.discard_until {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for p in packets {
            if p.is_empty() {
                continue;
            }
            for r in mtouch::decode(&p) {
                out.extend(mtouch::events(mtouch::Model::MTouch, &r));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    struct FakeKeypad {
        events: VecDeque<Result<Vec<Event>>>,
        leds: Rc<Cell<usize>>,
    }
    impl Surface for FakeKeypad {
        fn poll(&mut self) -> Result<Vec<Event>> {
            self.events.pop_front().unwrap_or_else(|| Ok(Vec::new()))
        }
        fn led(&mut self, _id: u16, _value: u16) {
            self.leds.set(self.leds.get() + 1);
        }
        fn name(&self) -> String {
            "fake NX-K".into()
        }
    }
    struct FakeMtouch {
        packets: VecDeque<Result<Vec<Vec<u8>>>>,
    }
    impl StripDevice for FakeMtouch {
        fn poll(&mut self) -> Result<Vec<Vec<u8>>> {
            self.packets.pop_front().unwrap_or_else(|| Ok(Vec::new()))
        }
        fn name(&self) -> String {
            "fake M-Touch".into()
        }
    }
    /// A base-fader report of strip 1: `02 02 lo hi 00 value touch`.
    fn strip1(value: u8, touch: bool) -> Vec<u8> {
        vec![0x02, 0x02, 0x12, 0x61, 0x00, value, touch as u8]
    }
    fn wait_discard() {
        std::thread::sleep(DISCARD_AFTER_OPEN + Duration::from_millis(20));
    }

    #[test]
    fn an_mtouch_only_installation_polls_its_strips_while_the_keypad_keeps_being_retried() {
        let attempts = Rc::new(Cell::new(0));
        let a = attempts.clone();
        let mut c: Combined<FakeKeypad, FakeMtouch> = Combined::new(
            Box::new(move || { a.set(a.get() + 1); Err(anyhow!("no NX-K found")) }),
            Some(Box::new(|| Ok(FakeMtouch { packets: VecDeque::from([Ok(vec![strip1(9, true)]), Ok(vec![strip1(100, true)]), Ok(vec![strip1(120, true), strip1(120, false)])]) }))),
        );
        assert!(c.poll().unwrap().is_empty(), "no keypad: no events, no error");
        assert_eq!(attempts.get(), 1);
        assert!(c.poll().unwrap().is_empty());
        assert_eq!(attempts.get(), 1, "the retry waits two seconds");
        assert!(!c.has_keypad());
        c.led(0x4401, 1);  // nowhere to go, no panic
        assert!(c.poll_strips().unwrap().is_empty(), "the open's queued history is discarded");
        assert!(c.has_mtouch());
        wait_discard();
        let ev = c.poll_strips().unwrap();
        assert_eq!(ev, vec![MtEvent::Fader { id: 0x6112, name: "VALUE BASE CHANNEL 1", value: 100, touch: true }]);
        let ev = c.poll_strips().unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(c.name(), "no NX-K + fake M-Touch");
    }

    #[test]
    fn losing_the_keypad_keeps_the_mtouch_running_and_a_reopened_keypad_is_announced_for_a_repaint() {
        let opens = Rc::new(Cell::new(0));
        let o = opens.clone();
        let leds = Rc::new(Cell::new(0));
        let l = leds.clone();
        let mut c: Combined<FakeKeypad, FakeMtouch> = Combined::new(
            Box::new(move || {
                o.set(o.get() + 1);
                let events = if o.get() == 1 { VecDeque::from([Ok(vec![Event::KeyDown { name: "Clear", id: 1 }]), Err(anyhow!("NX-K disconnected"))]) } else { VecDeque::new() };
                Ok(FakeKeypad { events, leds: l.clone() })
            }),
            Some(Box::new(|| Ok(FakeMtouch { packets: VecDeque::from([Ok(vec![]), Ok(vec![strip1(50, true)])]) }))),
        );
        assert_eq!(c.poll().unwrap(), vec![Event::KeyDown { name: "Clear", id: 1 }]);
        assert!(c.reconnected(), "the first open counts as a reconnect: the LEDs are painted");
        assert!(!c.reconnected());
        c.led(1, 1);
        assert_eq!(leds.get(), 1);
        assert!(c.poll_strips().unwrap().is_empty());
        assert!(c.poll().unwrap().is_empty(), "the keypad failed: dropped, no error");
        assert!(!c.has_keypad());
        assert!(c.disconnected(), "the loss is reported once so the loop releases what the keypad held");
        assert!(!c.disconnected());
        wait_discard();
        let ev = c.poll_strips().unwrap();
        assert_eq!(ev.len(), 1, "the M-Touch is unaffected by the keypad's loss");
        assert_eq!(c.name(), "no NX-K + fake M-Touch");
        c.led(1, 1);
        assert_eq!(leds.get(), 1, "no keypad: LED writes go nowhere");
        assert!(c.poll().unwrap().is_empty());
        assert_eq!(opens.get(), 1, "reopen waits two seconds");
        c.next_keypad = Instant::now();
        assert!(c.poll().unwrap().is_empty());
        assert_eq!(opens.get(), 2);
        assert!(c.has_keypad() && c.reconnected(), "reopened: the loop repaints the LEDs");
    }

    #[test]
    fn losing_the_mtouch_is_an_error_once_then_a_reopen() {
        let opens = Rc::new(Cell::new(0));
        let o = opens.clone();
        let mut c: Combined<FakeKeypad, FakeMtouch> = Combined::new(
            Box::new(|| Err(anyhow!("none"))),
            Some(Box::new(move || { o.set(o.get() + 1); Ok(FakeMtouch { packets: VecDeque::from([Err(anyhow!("M-Touch disconnected"))]) }) })),
        );
        assert!(c.poll_strips().is_err(), "the loss surfaces once so the loop can end the touches");
        assert!(!c.has_mtouch());
        assert!(c.poll_strips().unwrap().is_empty(), "then nothing until the reopen");
        assert_eq!(opens.get(), 1);
        c.next_mtouch = Instant::now();
        assert!(c.poll_strips().is_err());
        assert_eq!(opens.get(), 2);
        // Not wanted at all: never opened.
        let mut n: Combined<FakeKeypad, FakeMtouch> = Combined::new(Box::new(|| Err(anyhow!("none"))), None);
        assert!(n.poll_strips().unwrap().is_empty());
        assert_eq!(n.name(), "no NX-K, no M-Touch (retrying)");
    }
}
