//! Client-side application of received input, with bookkeeping that guarantees nothing
//! stays pressed: every key and button pressed through this state is released on
//! `Leave`, `ReleaseAll`, disconnect, or drop.

use std::collections::HashMap;

use crate::keys;
use crate::protocol::{Message, MouseButton, WheelAxis, key_flags};

/// OS-level injection primitives. Each returns false if the OS rejected the input
/// (e.g. UIPI blocked injection into an elevated window).
pub trait Injector: Send {
    fn move_abs(&mut self, x: i32, y: i32) -> bool;
    fn button(&mut self, button: MouseButton, down: bool) -> bool;
    fn wheel(&mut self, axis: WheelAxis, delta: i16) -> bool;
    /// `by_vk`: inject the virtual-key code instead of the scan code.
    fn key(&mut self, vk: u16, scan: u16, extended: bool, down: bool, by_vk: bool) -> bool;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ApplyResult {
    pub injected: u32,
    pub failed: u32,
}

pub struct InjectState<I: Injector> {
    inj: I,
    active: bool,
    keys: HashMap<u16, (u16, bool)>,
    buttons: u8,
}

impl<I: Injector> InjectState<I> {
    pub fn new(inj: I) -> Self {
        Self { inj, active: false, keys: HashMap::new(), buttons: 0 }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn injector(&mut self) -> &mut I {
        &mut self.inj
    }

    /// Applies one host message. Input messages are ignored while the shared cursor is
    /// not on this computer.
    pub fn apply(&mut self, msg: &Message) -> ApplyResult {
        let mut r = ApplyResult::default();
        let mut count = |ok: bool| if ok { r.injected += 1 } else { r.failed += 1 };
        match *msg {
            Message::Enter { x, y } => {
                self.active = true;
                count(self.inj.move_abs(x, y));
            }
            Message::Leave => {
                self.release_all_inner(&mut count);
                self.active = false;
            }
            Message::ReleaseAll => self.release_all_inner(&mut count),
            _ if !self.active => {}
            Message::MouseMove { x, y } => count(self.inj.move_abs(x, y)),
            Message::MouseButton { button, down } => {
                let bit = button.bit();
                if down {
                    self.buttons |= bit;
                } else {
                    self.buttons &= !bit;
                }
                count(self.inj.button(button, down));
            }
            Message::Wheel { axis, delta } => count(self.inj.wheel(axis, delta)),
            Message::Key { vk, scan, flags } => {
                let down = flags & key_flags::DOWN != 0;
                let extended = flags & key_flags::EXTENDED != 0;
                if down {
                    self.keys.insert(vk, (scan, extended));
                } else {
                    self.keys.remove(&vk);
                }
                count(self.inj.key(vk, scan, extended, down, keys::inject_by_vk(vk, scan)));
            }
            _ => {}
        }
        r
    }

    /// Releases every key and button pressed through this state.
    pub fn release_all(&mut self) -> ApplyResult {
        let mut r = ApplyResult::default();
        let mut count = |ok: bool| if ok { r.injected += 1 } else { r.failed += 1 };
        self.release_all_inner(&mut count);
        r
    }

    /// Release everything and mark inactive (connection lost).
    pub fn reset(&mut self) {
        self.release_all();
        self.active = false;
    }

    fn release_all_inner(&mut self, count: &mut impl FnMut(bool)) {
        // Release non-modifiers first so that e.g. Ctrl+C does not turn into a bare C.
        let mut held: Vec<(u16, (u16, bool))> = self.keys.drain().collect();
        held.sort_by_key(|(vk, _)| (keys::is_modifier(*vk), *vk));
        // Releasing a Win (or Alt) whose press saw no other key would open the Start menu
        // (or activate a menu bar) here, although the user was switching away. Tap the
        // unassigned mask key first so the shell sees a combination instead.
        if held.iter().any(|(vk, _)| keys::is_menu_modifier(*vk)) {
            count(self.inj.key(keys::VK_MASK, 0, false, true, true));
            count(self.inj.key(keys::VK_MASK, 0, false, false, true));
        }
        for (vk, (scan, ext)) in held {
            count(self.inj.key(vk, scan, ext, false, keys::inject_by_vk(vk, scan)));
        }
        for b in MouseButton::ALL {
            if self.buttons & b.bit() != 0 {
                count(self.inj.button(b, false));
            }
        }
        self.buttons = 0;
    }
}

impl<I: Injector> Drop for InjectState<I> {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Ev {
        Move(i32, i32),
        Button(MouseButton, bool),
        Wheel(WheelAxis, i16),
        Key(u16, bool, bool),
    }

    #[derive(Clone, Default)]
    pub struct Recorder(pub Arc<Mutex<Vec<Ev>>>);

    impl Injector for Recorder {
        fn move_abs(&mut self, x: i32, y: i32) -> bool {
            self.0.lock().unwrap().push(Ev::Move(x, y));
            true
        }
        fn button(&mut self, b: MouseButton, down: bool) -> bool {
            self.0.lock().unwrap().push(Ev::Button(b, down));
            true
        }
        fn wheel(&mut self, a: WheelAxis, d: i16) -> bool {
            self.0.lock().unwrap().push(Ev::Wheel(a, d));
            true
        }
        fn key(&mut self, vk: u16, _scan: u16, _ext: bool, down: bool, by_vk: bool) -> bool {
            self.0.lock().unwrap().push(Ev::Key(vk, down, by_vk));
            true
        }
    }

    #[test]
    fn input_is_ignored_until_enter() {
        let rec = Recorder::default();
        let mut s = InjectState::new(rec.clone());
        s.apply(&Message::MouseMove { x: 1, y: 1 });
        assert!(rec.0.lock().unwrap().is_empty());
        s.apply(&Message::Enter { x: 5, y: 6 });
        s.apply(&Message::MouseMove { x: 7, y: 8 });
        assert_eq!(*rec.0.lock().unwrap(), vec![Ev::Move(5, 6), Ev::Move(7, 8)]);
    }

    #[test]
    fn leave_releases_held_keys_and_buttons() {
        let rec = Recorder::default();
        let mut s = InjectState::new(rec.clone());
        s.apply(&Message::Enter { x: 0, y: 0 });
        s.apply(&Message::Key { vk: keys::VK_LCONTROL, scan: 0x1d, flags: key_flags::DOWN });
        s.apply(&Message::Key { vk: 0x43, scan: 0x2e, flags: key_flags::DOWN });
        s.apply(&Message::MouseButton { button: MouseButton::Left, down: true });
        rec.0.lock().unwrap().clear();
        s.apply(&Message::Leave);
        assert_eq!(
            *rec.0.lock().unwrap(),
            vec![
                Ev::Key(0x43, false, false),
                Ev::Key(keys::VK_LCONTROL, false, false),
                Ev::Button(MouseButton::Left, false)
            ]
        );
        assert!(!s.active());
        rec.0.lock().unwrap().clear();
        s.apply(&Message::ReleaseAll);
        assert!(rec.0.lock().unwrap().is_empty(), "nothing left to release");
    }

    #[test]
    fn releasing_win_on_leave_is_masked_so_start_does_not_open() {
        let rec = Recorder::default();
        let mut s = InjectState::new(rec.clone());
        s.apply(&Message::Enter { x: 0, y: 0 });
        s.apply(&Message::Key { vk: keys::VK_LWIN, scan: 0x5b, flags: key_flags::DOWN | key_flags::EXTENDED });
        rec.0.lock().unwrap().clear();
        s.apply(&Message::Leave);
        assert_eq!(
            *rec.0.lock().unwrap(),
            vec![
                Ev::Key(keys::VK_MASK, true, true),
                Ev::Key(keys::VK_MASK, false, true),
                Ev::Key(keys::VK_LWIN, false, false)
            ]
        );
        // A forwarded mask key (host-side shortcut hand-over) is injected by virtual key.
        s.apply(&Message::Enter { x: 0, y: 0 });
        rec.0.lock().unwrap().clear();
        s.apply(&Message::Key { vk: keys::VK_MASK, scan: 0, flags: key_flags::DOWN });
        assert_eq!(*rec.0.lock().unwrap(), vec![Ev::Key(keys::VK_MASK, true, true)]);
    }

    #[test]
    fn drop_releases_everything() {
        let rec = Recorder::default();
        {
            let mut s = InjectState::new(rec.clone());
            s.apply(&Message::Enter { x: 0, y: 0 });
            s.apply(&Message::Key { vk: 0x41, scan: 0x1e, flags: key_flags::DOWN });
        }
        assert_eq!(rec.0.lock().unwrap().last(), Some(&Ev::Key(0x41, false, false)));
    }

    #[test]
    fn media_keys_use_virtual_key_injection() {
        let rec = Recorder::default();
        let mut s = InjectState::new(rec.clone());
        s.apply(&Message::Enter { x: 0, y: 0 });
        s.apply(&Message::Key { vk: 0xAF, scan: 0x30, flags: key_flags::DOWN | key_flags::EXTENDED });
        assert_eq!(rec.0.lock().unwrap().last(), Some(&Ev::Key(0xAF, true, true)));
    }
}
