//! Host input router: the state machine that decides, for every captured input event,
//! whether it stays on the host or is forwarded to a client.
//!
//! It is deliberately free of OS calls. The capture layer feeds it raw events plus the
//! current cursor position, and executes the returned [`Action`]s (send to a client, warp
//! or hide the host cursor, ...). The returned [`Verdict`] tells the hook whether to let
//! the event through to the host or swallow it.
//!
//! While a client is active the host cursor is parked in the middle of the primary
//! monitor and every move is swallowed, so the OS never actually moves it. The movement
//! delta is the difference between the position the OS *would* have moved to and the
//! parked position; this keeps the host's pointer acceleration, so the cursor feels the
//! same on both machines.

use std::collections::HashMap;

use crate::geometry::{Layout, Point, Step, Target};
use crate::keys;
use crate::protocol::{key_flags, Message, MouseButton, WheelAxis};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawInput {
    /// Position the OS is about to move the cursor to (already clamped to the desktop).
    MouseMove { x: i32, y: i32 },
    MouseButton { button: MouseButton, down: bool },
    Wheel { axis: WheelAxis, delta: i16 },
    Key { vk: u16, scan: u16, extended: bool, down: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Let the event reach the host.
    Pass,
    /// Drop the event on the host.
    Swallow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Send { client: String, msg: Message },
    /// Move the host cursor (SetCursorPos).
    WarpCursor(Point),
    /// Hide the host cursor at the park position while a client is active.
    HideCursor(Point),
    ShowCursor,
    /// Keep the host display awake while it receives no local input.
    KeepAwake(bool),
    ActiveChanged(Option<String>),
    LockChanged(bool),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouterSettings {
    pub switch_delay_ms: u32,
    pub corner_dead_zone: i32,
    pub pointer_speed: f64,
    pub scroll_lock_toggle: bool,
}

impl Default for RouterSettings {
    fn default() -> Self {
        Self { switch_delay_ms: 0, corner_dead_zone: 0, pointer_speed: 1.0, scroll_lock_toggle: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Local,
    Remote { id: String, pos: Point, acc_x: f64, acc_y: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyInfo {
    scan: u16,
    extended: bool,
}

pub struct Router {
    layout: Layout,
    settings: RouterSettings,
    mode: Mode,
    /// Where the host cursor rests while a client is active.
    park: Point,
    /// Moves larger than this in one event are artefacts of a warp, not real motion.
    bogus_limit: i32,
    /// Keys physically held whose key-down reached the host.
    local_keys: HashMap<u16, KeyInfo>,
    /// Modifiers held when entering a client and mirrored to it.
    shared_mods: Vec<u16>,
    /// Keys whose key-down was forwarded to the active client.
    remote_keys: HashMap<u16, KeyInfo>,
    /// Keys consumed by a hotkey; their key-up is swallowed too.
    hotkey_keys: Vec<u16>,
    local_buttons: u8,
    remote_buttons: u8,
    locked: bool,
    /// (crossing destination, first time the cursor reached the edge), for switch delay.
    edge_since: Option<(Target, u64)>,
    /// Host position where the cursor left; used if the client vanishes.
    return_point: Point,
}

impl Router {
    pub fn new(layout: Layout, settings: RouterSettings) -> Self {
        let mut r = Self {
            park: Point::default(),
            bogus_limit: 200,
            layout,
            settings,
            mode: Mode::Local,
            local_keys: HashMap::new(),
            shared_mods: Vec::new(),
            remote_keys: HashMap::new(),
            hotkey_keys: Vec::new(),
            local_buttons: 0,
            remote_buttons: 0,
            locked: false,
            edge_since: None,
            return_point: Point::default(),
        };
        r.recompute_park();
        r
    }

    pub fn active(&self) -> Option<&str> {
        match &self.mode {
            Mode::Local => None,
            Mode::Remote { id, .. } => Some(id),
        }
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn settings(&self) -> RouterSettings {
        self.settings
    }

    pub fn set_settings(&mut self, s: RouterSettings) {
        self.settings = s;
        if !s.scroll_lock_toggle && self.locked {
            self.locked = false;
        }
    }

    /// Replaces the layout (monitors changed, a client joined/left or was moved).
    pub fn set_layout(&mut self, layout: Layout, out: &mut Vec<Action>) {
        self.layout = layout;
        self.recompute_park();
        if let Mode::Remote { id, pos, .. } = &mut self.mode {
            match self.layout.client_index(id) {
                Some(ci) => {
                    let screen = &self.layout.clients[ci].screen;
                    if !screen.contains(*pos) {
                        *pos = screen.clamp_nearest(*pos);
                        out.push(Action::Send { client: id.clone(), msg: Message::MouseMove { x: pos.x, y: pos.y } });
                    }
                }
                None => self.return_home(None, out),
            }
        }
        self.return_point = self.layout.host.clamp_nearest(self.return_point);
    }

    /// A client disconnected. If it held the cursor, bring the cursor home.
    pub fn client_gone(&mut self, id: &str, out: &mut Vec<Action>) {
        if self.active() == Some(id) {
            self.return_home(None, out);
        }
    }

    /// Moves the cursor to the centre of a client's primary monitor (layout test).
    pub fn switch_to(&mut self, id: &str, cursor: Point, out: &mut Vec<Action>) -> bool {
        let Some(ci) = self.layout.client_index(id) else { return false };
        if self.active() == Some(id) {
            return true;
        }
        let entry = self.layout.clients[ci].screen.monitors[0].center();
        match self.mode.clone() {
            Mode::Local => {
                self.return_point = cursor;
                self.enter(ci, entry, true, out);
            }
            Mode::Remote { id: old, .. } => {
                self.leave_client(&old, out);
                self.enter(ci, entry, false, out);
            }
        }
        true
    }

    /// Returns the cursor to the host (hotkey, UI, or shutdown).
    pub fn force_local(&mut self, out: &mut Vec<Action>) {
        if let Mode::Remote { id, .. } = self.mode.clone() {
            self.leave_client(&id, out);
            self.return_home(None, out);
        }
    }

    /// Feeds one captured event. `cursor` is the host cursor position right now
    /// (before this event is applied), `now_ms` a monotonic clock.
    pub fn handle(&mut self, input: RawInput, cursor: Point, now_ms: u64, out: &mut Vec<Action>) -> Verdict {
        match input {
            RawInput::MouseMove { x, y } => self.on_move(Point::new(x, y), cursor, now_ms, out),
            RawInput::MouseButton { button, down } => self.on_button(button, down, out),
            RawInput::Wheel { axis, delta } => match &self.mode {
                Mode::Local => Verdict::Pass,
                Mode::Remote { id, .. } => {
                    out.push(Action::Send { client: id.clone(), msg: Message::Wheel { axis, delta } });
                    Verdict::Swallow
                }
            },
            RawInput::Key { vk, scan, extended, down } => self.on_key(vk, KeyInfo { scan, extended }, down, out),
        }
    }

    fn recompute_park(&mut self) {
        let primary = self.layout.host.monitors[0];
        self.park = primary.center();
        self.bogus_limit = (primary.w.min(primary.h) / 3).max(100);
    }

    fn on_move(&mut self, p: Point, cursor: Point, now_ms: u64, out: &mut Vec<Action>) -> Verdict {
        match self.mode.clone() {
            Mode::Local => {
                if self.locked || self.local_buttons != 0 {
                    self.edge_since = None;
                    return Verdict::Pass;
                }
                let Some(c) = self.layout.host_edge_exit(p, self.settings.corner_dead_zone) else {
                    self.edge_since = None;
                    return Verdict::Pass;
                };
                if !self.delay_elapsed(c.to, now_ms) {
                    return Verdict::Pass;
                }
                let Target::Client(ci) = c.to else { return Verdict::Pass };
                self.return_point = p;
                self.enter(ci, c.point, true, out);
                Verdict::Swallow
            }
            Mode::Remote { id, pos, acc_x, acc_y } => {
                let dx = p.x - cursor.x;
                let dy = p.y - cursor.y;
                if dx.abs() > self.bogus_limit || dy.abs() > self.bogus_limit {
                    // An event generated before the cursor was parked; ignore it.
                    if (cursor.x - self.park.x).abs() > self.bogus_limit / 2
                        || (cursor.y - self.park.y).abs() > self.bogus_limit / 2
                    {
                        out.push(Action::WarpCursor(self.park));
                    }
                    return Verdict::Swallow;
                }
                if dx == 0 && dy == 0 {
                    return Verdict::Swallow;
                }
                let Some(ci) = self.layout.client_index(&id) else {
                    self.return_home(None, out);
                    return Verdict::Swallow;
                };
                // Convert host pixels to client pixels, keeping sub-pixel remainders.
                let ratio = self.settings.pointer_speed / self.layout.clients[ci].scale;
                let fx = acc_x + dx as f64 * ratio;
                let fy = acc_y + dy as f64 * ratio;
                let (sx, sy) = (fx.trunc(), fy.trunc());
                let (acc_x, acc_y) = (fx - sx, fy - sy);
                let (sx, sy) = (sx as i32, sy as i32);
                if sx == 0 && sy == 0 {
                    self.mode = Mode::Remote { id, pos, acc_x, acc_y };
                    return Verdict::Swallow;
                }
                let step = if self.locked || self.remote_buttons != 0 {
                    let screen = &self.layout.clients[ci].screen;
                    Step::Stay(screen.clamp_nearest(Point::new(pos.x + sx, pos.y + sy)))
                } else {
                    self.layout.client_step(ci, pos, sx, sy, self.settings.corner_dead_zone)
                };
                match step {
                    Step::Stay(q) => {
                        self.edge_since = None;
                        if q != pos {
                            out.push(Action::Send { client: id.clone(), msg: Message::MouseMove { x: q.x, y: q.y } });
                        }
                        self.mode = Mode::Remote { id, pos: q, acc_x, acc_y };
                    }
                    Step::Cross(c) => {
                        if !self.delay_elapsed(c.to, now_ms) {
                            // Hold the cursor on the edge until the delay passes.
                            self.mode = Mode::Remote { id, pos, acc_x: 0.0, acc_y: 0.0 };
                            return Verdict::Swallow;
                        }
                        self.leave_client(&id, out);
                        match c.to {
                            Target::Host => self.return_home(Some(c.point), out),
                            Target::Client(cj) => self.enter(cj, c.point, false, out),
                        }
                    }
                }
                Verdict::Swallow
            }
        }
    }

    fn delay_elapsed(&mut self, to: Target, now_ms: u64) -> bool {
        if self.settings.switch_delay_ms == 0 {
            return true;
        }
        match self.edge_since {
            Some((t, since)) if t == to => {
                if now_ms.saturating_sub(since) >= self.settings.switch_delay_ms as u64 {
                    self.edge_since = None;
                    true
                } else {
                    false
                }
            }
            _ => {
                self.edge_since = Some((to, now_ms));
                false
            }
        }
    }

    /// Makes client `ci` active with the cursor at `entry`.
    fn enter(&mut self, ci: usize, entry: Point, from_host: bool, out: &mut Vec<Action>) {
        let id = self.layout.clients[ci].id.clone();
        if from_host {
            out.push(Action::KeepAwake(true));
            out.push(Action::HideCursor(self.park));
            out.push(Action::WarpCursor(self.park));
        }
        out.push(Action::Send { client: id.clone(), msg: Message::Enter { x: entry.x, y: entry.y } });
        // Mirror held modifiers so Shift/Ctrl held while crossing keep working.
        self.shared_mods.clear();
        let mut mods: Vec<(u16, KeyInfo)> =
            self.local_keys.iter().filter(|(vk, _)| keys::is_modifier(**vk)).map(|(vk, k)| (*vk, *k)).collect();
        mods.sort_by_key(|(vk, _)| *vk);
        for (vk, k) in mods {
            out.push(Action::Send { client: id.clone(), msg: key_msg(vk, k, true) });
            self.shared_mods.push(vk);
        }
        self.remote_keys.clear();
        self.remote_buttons = 0;
        self.edge_since = None;
        self.mode = Mode::Remote { id: id.clone(), pos: entry, acc_x: 0.0, acc_y: 0.0 };
        out.push(Action::ActiveChanged(Some(id)));
    }

    /// Tells the active client to release everything; does not change the host cursor.
    fn leave_client(&mut self, id: &str, out: &mut Vec<Action>) {
        out.push(Action::Send { client: id.to_owned(), msg: Message::Leave });
        self.remote_keys.clear();
        self.shared_mods.clear();
        self.remote_buttons = 0;
    }

    /// Brings the cursor back to the host at `at` (or where it left).
    fn return_home(&mut self, at: Option<Point>, out: &mut Vec<Action>) {
        let p = self.layout.host.clamp_nearest(at.unwrap_or(self.return_point));
        self.mode = Mode::Local;
        self.remote_keys.clear();
        self.shared_mods.clear();
        self.remote_buttons = 0;
        self.edge_since = None;
        out.push(Action::ShowCursor);
        out.push(Action::WarpCursor(p));
        out.push(Action::KeepAwake(false));
        out.push(Action::ActiveChanged(None));
    }

    fn on_button(&mut self, button: MouseButton, down: bool, out: &mut Vec<Action>) -> Verdict {
        let bit = button.bit();
        match &self.mode {
            Mode::Local => {
                if down {
                    self.local_buttons |= bit;
                } else {
                    self.local_buttons &= !bit;
                }
                Verdict::Pass
            }
            Mode::Remote { id, .. } => {
                if !down && self.local_buttons & bit != 0 {
                    // Pressed before the switch: release it where it was pressed.
                    self.local_buttons &= !bit;
                    return Verdict::Pass;
                }
                if down {
                    self.remote_buttons |= bit;
                } else {
                    self.remote_buttons &= !bit;
                }
                out.push(Action::Send { client: id.clone(), msg: Message::MouseButton { button, down } });
                Verdict::Swallow
            }
        }
    }

    fn on_key(&mut self, vk: u16, info: KeyInfo, down: bool, out: &mut Vec<Action>) -> Verdict {
        // Hotkeys are evaluated on the physical key state regardless of where input goes.
        if down {
            if self.hotkey_keys.contains(&vk) {
                return Verdict::Swallow; // auto-repeat of a hotkey key
            }
            if vk == keys::VK_SCROLL && self.settings.scroll_lock_toggle {
                self.hotkey_keys.push(vk);
                self.locked = !self.locked;
                self.edge_since = None;
                out.push(Action::LockChanged(self.locked));
                return Verdict::Swallow;
            }
            if vk == keys::VK_ESCAPE && self.active().is_some() && self.ctrl_alt_held() {
                self.hotkey_keys.push(vk);
                self.force_local(out);
                return Verdict::Swallow;
            }
        } else if let Some(i) = self.hotkey_keys.iter().position(|k| *k == vk) {
            self.hotkey_keys.swap_remove(i);
            return Verdict::Swallow;
        }

        match &self.mode {
            Mode::Local => {
                if down {
                    self.local_keys.insert(vk, info);
                } else {
                    self.local_keys.remove(&vk);
                }
                Verdict::Pass
            }
            Mode::Remote { id, .. } => {
                let id = id.clone();
                if down {
                    if self.local_keys.contains_key(&vk) {
                        // Auto-repeat of a key held since before the switch.
                        return Verdict::Swallow;
                    }
                    self.remote_keys.insert(vk, info);
                    out.push(Action::Send { client: id, msg: key_msg(vk, info, true) });
                    Verdict::Swallow
                } else if self.local_keys.remove(&vk).is_some() {
                    if let Some(i) = self.shared_mods.iter().position(|k| *k == vk) {
                        self.shared_mods.swap_remove(i);
                        out.push(Action::Send { client: id, msg: key_msg(vk, info, false) });
                    }
                    Verdict::Pass
                } else {
                    // Forward every key-up, even without a matching down: an extra up is
                    // harmless, a missing one leaves a stuck key.
                    self.remote_keys.remove(&vk);
                    out.push(Action::Send { client: id, msg: key_msg(vk, info, false) });
                    Verdict::Swallow
                }
            }
        }
    }

    fn ctrl_alt_held(&self) -> bool {
        let held = |vks: &[u16]| vks.iter().any(|vk| self.local_keys.contains_key(vk) || self.remote_keys.contains_key(vk));
        held(&[keys::VK_LCONTROL, keys::VK_RCONTROL, keys::VK_CONTROL])
            && held(&[keys::VK_LMENU, keys::VK_RMENU, keys::VK_MENU])
    }
}

fn key_msg(vk: u16, k: KeyInfo, down: bool) -> Message {
    let mut flags = 0;
    if down {
        flags |= key_flags::DOWN;
    }
    if k.extended {
        flags |= key_flags::EXTENDED;
    }
    Message::Key { vk, scan: k.scan, flags }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ClientPlacement, Rect, Screen};

    const LAPTOP: &str = "laptop";

    fn desk() -> Layout {
        Layout {
            host: Screen::new(vec![Rect::new(0, 0, 2560, 1440), Rect::new(2560, 0, 2560, 1440)]),
            clients: vec![ClientPlacement {
                id: LAPTOP.into(),
                screen: Screen::new(vec![Rect::new(0, 0, 1920, 1200)]),
                origin_x: -1920.0,
                origin_y: 0.0,
                scale: 1.0,
            }],
        }
    }

    fn router() -> Router {
        Router::new(desk(), RouterSettings::default())
    }

    fn sent(out: &[Action]) -> Vec<Message> {
        out.iter()
            .filter_map(|a| match a {
                Action::Send { msg, .. } => Some(msg.clone()),
                _ => None,
            })
            .collect()
    }

    /// Pushes the cursor against the left edge of monitor 1 at height `y`.
    fn cross_left(r: &mut Router, y: i32) -> Vec<Action> {
        let mut out = Vec::new();
        let v = r.handle(RawInput::MouseMove { x: 0, y }, Point::new(1, y), 0, &mut out);
        assert_eq!(v, Verdict::Swallow);
        out
    }

    fn key(r: &mut Router, vk: u16, down: bool) -> (Verdict, Vec<Message>) {
        let mut out = Vec::new();
        let v = r.handle(RawInput::Key { vk, scan: vk, extended: false, down }, Point::new(1280, 720), 0, &mut out);
        (v, sent(&out))
    }

    #[test]
    fn local_moves_pass_through() {
        let mut r = router();
        let mut out = Vec::new();
        assert_eq!(r.handle(RawInput::MouseMove { x: 100, y: 100 }, Point::new(99, 100), 0, &mut out), Verdict::Pass);
        assert!(out.is_empty());
    }

    #[test]
    fn crossing_parks_cursor_and_enters_client() {
        let mut r = router();
        let out = cross_left(&mut r, 600);
        assert_eq!(r.active(), Some(LAPTOP));
        assert!(out.contains(&Action::HideCursor(Point::new(1280, 720))));
        assert!(out.contains(&Action::WarpCursor(Point::new(1280, 720))));
        assert!(out.contains(&Action::KeepAwake(true)));
        assert_eq!(sent(&out), vec![Message::Enter { x: 1919, y: 600 }]);
    }

    #[test]
    fn remote_moves_are_relative_to_the_parked_cursor() {
        let mut r = router();
        cross_left(&mut r, 600);
        let mut out = Vec::new();
        let park = Point::new(1280, 720);
        assert_eq!(r.handle(RawInput::MouseMove { x: 1270, y: 725 }, park, 0, &mut out), Verdict::Swallow);
        assert_eq!(sent(&out), vec![Message::MouseMove { x: 1909, y: 605 }]);
    }

    #[test]
    fn warp_artefacts_are_ignored() {
        let mut r = router();
        cross_left(&mut r, 600);
        let mut out = Vec::new();
        // A stale event from the edge position, delivered after the park warp.
        r.handle(RawInput::MouseMove { x: 0, y: 601 }, Point::new(1280, 720), 0, &mut out);
        assert!(sent(&out).is_empty());
    }

    #[test]
    fn moving_back_returns_to_host() {
        let mut r = router();
        cross_left(&mut r, 600);
        let mut out = Vec::new();
        r.handle(RawInput::MouseMove { x: 1290, y: 720 }, Point::new(1280, 720), 0, &mut out);
        assert_eq!(r.active(), None);
        assert_eq!(sent(&out), vec![Message::Leave]);
        assert!(out.contains(&Action::ShowCursor));
        assert!(out.contains(&Action::WarpCursor(Point::new(1, 600))));
        assert!(out.contains(&Action::ActiveChanged(None)));
    }

    #[test]
    fn keys_follow_the_active_computer() {
        let mut r = router();
        assert_eq!(key(&mut r, 0x41, true).0, Verdict::Pass);
        assert_eq!(key(&mut r, 0x41, false).0, Verdict::Pass);
        cross_left(&mut r, 600);
        let (v, m) = key(&mut r, 0x41, true);
        assert_eq!(v, Verdict::Swallow);
        assert_eq!(m, vec![Message::Key { vk: 0x41, scan: 0x41, flags: key_flags::DOWN }]);
        let (v, m) = key(&mut r, 0x41, false);
        assert_eq!(v, Verdict::Swallow);
        assert_eq!(m, vec![Message::Key { vk: 0x41, scan: 0x41, flags: 0 }]);
    }

    #[test]
    fn key_held_across_switch_is_released_on_the_host() {
        let mut r = router();
        key(&mut r, 0x41, true);
        cross_left(&mut r, 600);
        // Auto-repeat while remote is dropped, the release goes to the host.
        assert_eq!(key(&mut r, 0x41, true), (Verdict::Swallow, vec![]));
        assert_eq!(key(&mut r, 0x41, false), (Verdict::Pass, vec![]));
    }

    #[test]
    fn modifiers_held_while_crossing_are_mirrored() {
        let mut r = router();
        key(&mut r, keys::VK_LSHIFT, true);
        let out = cross_left(&mut r, 600);
        assert_eq!(
            sent(&out),
            vec![
                Message::Enter { x: 1919, y: 600 },
                Message::Key { vk: keys::VK_LSHIFT, scan: keys::VK_LSHIFT, flags: key_flags::DOWN }
            ]
        );
        // Releasing it releases it on both computers.
        let (v, m) = key(&mut r, keys::VK_LSHIFT, false);
        assert_eq!(v, Verdict::Pass);
        assert_eq!(m, vec![Message::Key { vk: keys::VK_LSHIFT, scan: keys::VK_LSHIFT, flags: 0 }]);
    }

    #[test]
    fn no_switch_while_a_button_is_held() {
        let mut r = router();
        let mut out = Vec::new();
        r.handle(RawInput::MouseButton { button: MouseButton::Left, down: true }, Point::new(1, 600), 0, &mut out);
        assert_eq!(r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(1, 600), 0, &mut out), Verdict::Pass);
        assert_eq!(r.active(), None);
        r.handle(RawInput::MouseButton { button: MouseButton::Left, down: false }, Point::new(0, 600), 0, &mut out);
        assert_eq!(r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(0, 600), 0, &mut out), Verdict::Swallow);
        assert_eq!(r.active(), Some(LAPTOP));
    }

    #[test]
    fn remote_drag_stays_on_client() {
        let mut r = router();
        cross_left(&mut r, 600);
        let park = Point::new(1280, 720);
        let mut out = Vec::new();
        r.handle(RawInput::MouseButton { button: MouseButton::Left, down: true }, park, 0, &mut out);
        // Pushing right at the laptop's edge would normally return to the host.
        r.handle(RawInput::MouseMove { x: 1290, y: 720 }, park, 0, &mut out);
        assert_eq!(r.active(), Some(LAPTOP));
        assert_eq!(sent(&out), vec![Message::MouseButton { button: MouseButton::Left, down: true }]);
    }

    #[test]
    fn buttons_and_wheel_are_forwarded() {
        let mut r = router();
        cross_left(&mut r, 600);
        let park = Point::new(1280, 720);
        let mut out = Vec::new();
        r.handle(RawInput::MouseButton { button: MouseButton::Right, down: true }, park, 0, &mut out);
        r.handle(RawInput::MouseButton { button: MouseButton::Right, down: false }, park, 0, &mut out);
        r.handle(RawInput::Wheel { axis: WheelAxis::Vertical, delta: -120 }, park, 0, &mut out);
        assert_eq!(
            sent(&out),
            vec![
                Message::MouseButton { button: MouseButton::Right, down: true },
                Message::MouseButton { button: MouseButton::Right, down: false },
                Message::Wheel { axis: WheelAxis::Vertical, delta: -120 },
            ]
        );
    }

    #[test]
    fn ctrl_alt_esc_returns_home() {
        let mut r = router();
        cross_left(&mut r, 600);
        key(&mut r, keys::VK_LCONTROL, true);
        key(&mut r, keys::VK_LMENU, true);
        let (v, m) = key(&mut r, keys::VK_ESCAPE, true);
        assert_eq!(v, Verdict::Swallow);
        assert_eq!(m, vec![Message::Leave]);
        assert_eq!(r.active(), None);
        // The Esc release is swallowed; Ctrl/Alt releases reach the host harmlessly.
        assert_eq!(key(&mut r, keys::VK_ESCAPE, false).0, Verdict::Swallow);
        assert_eq!(key(&mut r, keys::VK_LMENU, false).0, Verdict::Pass);
    }

    #[test]
    fn scroll_lock_locks_the_cursor() {
        let mut r = router();
        let (v, _) = key(&mut r, keys::VK_SCROLL, true);
        assert_eq!(v, Verdict::Swallow);
        key(&mut r, keys::VK_SCROLL, false);
        assert!(r.locked());
        let mut out = Vec::new();
        assert_eq!(r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(1, 600), 0, &mut out), Verdict::Pass);
        assert_eq!(r.active(), None);
    }

    #[test]
    fn switch_delay_requires_resting_on_the_edge() {
        let mut r = Router::new(desk(), RouterSettings { switch_delay_ms: 150, ..Default::default() });
        let mut out = Vec::new();
        let ev = RawInput::MouseMove { x: 0, y: 600 };
        assert_eq!(r.handle(ev, Point::new(0, 600), 1000, &mut out), Verdict::Pass);
        assert_eq!(r.handle(ev, Point::new(0, 600), 1100, &mut out), Verdict::Pass);
        assert_eq!(r.handle(ev, Point::new(0, 600), 1160, &mut out), Verdict::Swallow);
        assert_eq!(r.active(), Some(LAPTOP));
    }

    #[test]
    fn disconnect_while_active_brings_cursor_home() {
        let mut r = router();
        cross_left(&mut r, 600);
        let mut out = Vec::new();
        r.client_gone(LAPTOP, &mut out);
        assert_eq!(r.active(), None);
        assert!(sent(&out).is_empty());
        assert!(out.contains(&Action::WarpCursor(Point::new(0, 600))));
        assert!(out.contains(&Action::ShowCursor));
    }

    #[test]
    fn pointer_speed_accumulates_fractions() {
        let mut layout = desk();
        layout.clients[0].scale = 2.0; // a 50 % client: half a client pixel per host pixel
        let mut r = Router::new(layout, RouterSettings::default());
        let mut out = Vec::new();
        r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(1, 600), 0, &mut out);
        out.clear();
        let park = Point::new(1280, 720);
        r.handle(RawInput::MouseMove { x: 1279, y: 720 }, park, 0, &mut out);
        assert!(sent(&out).is_empty(), "half a pixel does not move yet");
        r.handle(RawInput::MouseMove { x: 1279, y: 720 }, park, 0, &mut out);
        assert_eq!(sent(&out), vec![Message::MouseMove { x: 1918, y: 300 }]);
    }

    #[test]
    fn test_switch_and_force_local() {
        let mut r = router();
        let mut out = Vec::new();
        assert!(r.switch_to(LAPTOP, Point::new(500, 500), &mut out));
        assert_eq!(r.active(), Some(LAPTOP));
        assert_eq!(sent(&out), vec![Message::Enter { x: 960, y: 600 }]);
        out.clear();
        r.force_local(&mut out);
        assert_eq!(r.active(), None);
        assert!(out.contains(&Action::WarpCursor(Point::new(500, 500))));
    }
}
