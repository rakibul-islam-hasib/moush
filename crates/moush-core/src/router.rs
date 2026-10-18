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
//!
//! ## Keyboard
//!
//! Every physical key gets a destination when it goes down ([`Route`]) and its release
//! always goes to the same place, so no computer ever sees a press without its release.
//! The table is a fixed array indexed by virtual-key code: no allocation per keystroke.
//!
//! A *shortcut* (a non-modifier key pressed while modifiers are held) is routed as a
//! whole: the first non-modifier key decides where it goes, and every further key goes
//! there too until the last modifier is released. When a shortcut that has to stay on
//! the host (a local shortcut, or a system shortcut while those are not forwarded) is
//! typed while the cursor is on a client, the modifiers already forwarded are released
//! on the client and re-pressed on the host (injected) before the key, so neither side
//! sees half a shortcut. While a shortcut with Alt or Win is held (Alt+Tab switcher, Win+Tab)
//! the cursor does not cross to another computer, because that UI is driven with the
//! mouse on the computer that shows it.
//!
//! A lone Win press-and-release opens the Start menu and a lone Alt press-and-release
//! activates the menu bar. Whenever events between such a press and its release are
//! swallowed on one computer (because they went to another), that computer gets a
//! harmless "mask" key ([`keys::VK_MASK`]) before the release, so nothing pops up there.

use crate::geometry::{Edge, Layout, Point, Step, Target};
use crate::keys::{self, mods, ShortcutSet};
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
    /// Inject a key event on the host (by virtual-key code, tagged so that our own hooks
    /// ignore it). Used to hand a shortcut over to the host, to re-press modifiers still
    /// held when the cursor comes home, and to mask Win/Alt releases.
    LocalKey { vk: u16, scan: u16, extended: bool, down: bool },
}

/// Modifier that must be held for the cursor to cross a screen edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeModifier {
    #[default]
    None,
    Ctrl,
    Shift,
    Alt,
}

impl EdgeModifier {
    /// "none" | "ctrl" | "shift" | "alt" (anything else: none).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "ctrl" => Self::Ctrl,
            "shift" => Self::Shift,
            "alt" => Self::Alt,
            _ => Self::None,
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Ctrl => mods::CTRL,
            Self::Shift => mods::SHIFT,
            Self::Alt => mods::ALT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouterSettings {
    pub switch_delay_ms: u32,
    pub corner_dead_zone: i32,
    pub pointer_speed: f64,
    pub scroll_lock_toggle: bool,
    /// Forward the keyboard to the active client. Off: only the mouse moves between
    /// computers; typing stays on the host (Moush's hotkeys still work).
    pub forward_keyboard: bool,
    /// Alt+Tab, Win, Win+<key>, Alt+F4, Alt+Esc, Ctrl+Esc, Ctrl+Shift+Esc go to the active
    /// client. Off: they act on the host even while the cursor is on a client.
    pub forward_system_shortcuts: bool,
    /// Shortcuts that always act on the host.
    pub local_shortcuts: ShortcutSet,
    /// Ctrl+Alt+Shift+Arrow switches to the neighbouring computer, Ctrl+Alt+Shift+Home to the host.
    pub switch_hotkeys: bool,
    /// Screen edges only switch computers while this modifier is held.
    pub edge_modifier: EdgeModifier,
}

impl Default for RouterSettings {
    fn default() -> Self {
        Self {
            switch_delay_ms: 0,
            corner_dead_zone: 0,
            pointer_speed: 1.0,
            scroll_lock_toggle: true,
            forward_keyboard: true,
            forward_system_shortcuts: true,
            local_shortcuts: ShortcutSet::default(),
            switch_hotkeys: true,
            edge_modifier: EdgeModifier::None,
        }
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

/// Where a held key's press went (and therefore where its release goes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// The host saw the press (physically or injected by us).
    Local,
    /// Forwarded to the active client.
    Remote,
    /// Reached no computer (a Moush hotkey), or was released on a client that is no
    /// longer active. The release is swallowed.
    Dropped,
}

#[derive(Clone, Copy, Debug)]
struct Held {
    route: Route,
    info: KeyInfo,
    /// A host key whose press was also sent to the active client (modifier held while
    /// crossing, or joined to a shortcut typed there); its release goes to both.
    mirrored: bool,
    /// Auto-repeat reaches the key's destination. Off for host keys held while the cursor
    /// moved to a client, so they stop repeating on the host.
    repeat: bool,
    /// Host Win/Alt only: the mask key was already injected since it was pressed.
    masked: bool,
    /// Monotonic ms of the press or the latest auto-repeat.
    seen_ms: u64,
}

/// A "repeat" this long after the previous press or repeat of the same key means its
/// release was missed (input the hooks never saw); it is treated as a new press. Longer
/// than any keyboard repeat delay or interval Windows can be configured with (2 s).
const STALE_MS: u64 = 5000;

pub struct Router {
    layout: Layout,
    settings: RouterSettings,
    mode: Mode,
    /// Where the host cursor rests while a client is active.
    park: Point,
    /// Moves larger than this in one event are artefacts of a warp, not real motion.
    bogus_limit: i32,
    /// Held keys by virtual-key code.
    keys: [Option<Held>; 256],
    /// Destination of the shortcut in progress (Local or Remote), until all modifiers are up.
    combo: Option<Route>,
    local_buttons: u8,
    remote_buttons: u8,
    locked: bool,
    /// (crossing destination, first time the cursor reached the edge), for switch delay.
    edge_since: Option<(Target, u64)>,
    /// Host position where the cursor left; used if the client vanishes.
    return_point: Point,
    /// Latest host cursor position seen while the cursor was on the host.
    last_local: Point,
}

impl Router {
    pub fn new(layout: Layout, settings: RouterSettings) -> Self {
        let mut r = Self {
            park: Point::default(),
            bogus_limit: 200,
            last_local: layout.host.monitors[0].center(),
            layout,
            settings,
            mode: Mode::Local,
            keys: [None; 256],
            combo: None,
            local_buttons: 0,
            remote_buttons: 0,
            locked: false,
            edge_since: None,
            return_point: Point::default(),
        };
        r.recompute_park();
        r.return_point = r.last_local;
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

    /// Applies new settings. Keys already held keep their destination, so changing any
    /// keyboard setting while keys are down cannot leave a key stuck anywhere.
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
                None => {
                    // Still connected maybe, but no longer placed: release everything there.
                    let id = id.clone();
                    self.leave_client(&id, out);
                    self.return_home(None, false, out);
                }
            }
        }
        self.return_point = self.layout.host.clamp_nearest(self.return_point);
        self.last_local = self.layout.host.clamp_nearest(self.last_local);
    }

    /// A client disconnected. If it held the cursor, bring the cursor home.
    pub fn client_gone(&mut self, id: &str, out: &mut Vec<Action>) {
        if self.active() == Some(id) {
            self.return_home(None, false, out);
        }
    }

    /// Moves the cursor to the centre of a client's primary monitor (layout test).
    pub fn switch_to(&mut self, id: &str, cursor: Point, out: &mut Vec<Action>) -> bool {
        let Some(ci) = self.layout.client_index(id) else { return false };
        if matches!(self.mode, Mode::Local) {
            self.last_local = cursor;
        }
        self.jump_to(ci, out);
        true
    }

    /// Returns the cursor to the host (hotkey, UI, or shutdown).
    pub fn force_local(&mut self, out: &mut Vec<Action>) {
        if let Mode::Remote { id, .. } = self.mode.clone() {
            self.leave_client(&id, out);
            self.return_home(None, false, out);
        }
    }

    /// The host session was locked or unlocked. The secure desktop receives keyboard and
    /// mouse input that our hooks never see (e.g. the Win and L releases of Win+L), so
    /// every held key and button is forgotten after bringing the cursor home.
    pub fn session_changed(&mut self, out: &mut Vec<Action>) {
        self.force_local(out);
        self.keys = [None; 256];
        self.combo = None;
        self.local_buttons = 0;
        self.edge_since = None;
    }

    /// The hooks were bypassed for a while (Windows dropped them, or another hook ate
    /// events) and have been re-installed: key releases may have been missed. Everything
    /// forwarded to the active client is released there; host keys are kept only if
    /// `host_key_down` (the OS key state) says they are still down.
    pub fn resync_after_gap(&mut self, host_key_down: impl Fn(u16) -> bool, out: &mut Vec<Action>) {
        let forwarded = self.keys.iter().flatten().any(|h| h.route != Route::Local || h.mirrored);
        if forwarded {
            self.send(Message::ReleaseAll, out);
        }
        for (vk, slot) in self.keys.iter_mut().enumerate() {
            match slot {
                Some(h) if h.route == Route::Local && host_key_down(vk as u16) => h.mirrored = false,
                _ => *slot = None,
            }
        }
        self.combo = None;
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
                    self.mask_host(out);
                    Verdict::Swallow
                }
            },
            RawInput::Key { vk, scan, extended, down } => {
                self.on_key(vk, KeyInfo { scan, extended }, down, now_ms, out)
            }
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
                self.last_local = p;
                if self.locked || self.local_buttons != 0 || !self.edge_allowed() {
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
                self.enter(ci, c.point, true, true, out);
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
                    self.leave_client(&id, out);
                    self.return_home(None, false, out);
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
                let step = if self.locked || self.remote_buttons != 0 || !self.edge_allowed() {
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
                            Target::Host => self.return_home(Some(c.point), true, out),
                            Target::Client(cj) => self.enter(cj, c.point, false, true, out),
                        }
                    }
                }
                Verdict::Swallow
            }
        }
    }

    /// Edge crossings need the configured modifier, and never happen while a shortcut with
    /// Alt or Win is held (its switcher UI is used with the mouse where it is shown).
    fn edge_allowed(&self) -> bool {
        let held = self.held_mods();
        if self.combo.is_some() && held & (mods::ALT | mods::WIN) != 0 {
            return false;
        }
        let need = self.settings.edge_modifier.bit();
        need == 0 || held & need != 0
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

    /// Makes client `ci` active with the cursor at `entry`. `mirror`: the cursor crossed an
    /// edge, so held Shift/Ctrl are pressed on the client too (Shift+click across the edge).
    fn enter(&mut self, ci: usize, entry: Point, from_host: bool, mirror: bool, out: &mut Vec<Action>) {
        let id = self.layout.clients[ci].id.clone();
        if from_host {
            out.push(Action::KeepAwake(true));
            out.push(Action::HideCursor(self.park));
            out.push(Action::WarpCursor(self.park));
        }
        out.push(Action::Send { client: id.clone(), msg: Message::Enter { x: entry.x, y: entry.y } });
        self.mode = Mode::Remote { id: id.clone(), pos: entry, acc_x: 0.0, acc_y: 0.0 };
        self.combo = None;
        self.remote_buttons = 0;
        self.edge_since = None;
        if self.settings.forward_keyboard {
            // Host keys stop auto-repeating on the host once typing goes elsewhere.
            for h in self.keys.iter_mut().flatten() {
                if h.route == Route::Local {
                    h.repeat = false;
                }
            }
            if mirror {
                let skip = self.settings.edge_modifier.bit();
                for vk in keys::MODIFIER_VKS {
                    let bit = keys::modifier_bit(vk);
                    if bit & (mods::SHIFT | mods::CTRL) == 0 || bit == skip {
                        continue;
                    }
                    if let Some(mut h) = self.keys[vk as usize] {
                        match h.route {
                            Route::Local => h.mirrored = true,
                            Route::Dropped => h.route = Route::Remote,
                            Route::Remote => continue,
                        }
                        out.push(Action::Send { client: id.clone(), msg: key_msg(vk, h.info, true) });
                        self.keys[vk as usize] = Some(h);
                    }
                }
            }
        }
        if from_host {
            // Win/Alt held on the host while the cursor leaves: their release must not
            // open the Start menu or a menu bar there.
            self.mask_host(out);
        }
        out.push(Action::ActiveChanged(Some(id)));
    }

    /// Tells the active client to release everything (the client masks Win/Alt before
    /// releasing them); does not change the host cursor.
    fn leave_client(&mut self, id: &str, out: &mut Vec<Action>) {
        out.push(Action::Send { client: id.to_owned(), msg: Message::Leave });
        self.forget_remote();
        self.remote_buttons = 0;
    }

    /// Keys forwarded to the client that is being left were released there; their own
    /// releases are swallowed later.
    fn forget_remote(&mut self) {
        for h in self.keys.iter_mut().flatten() {
            if h.route == Route::Remote {
                h.route = Route::Dropped;
            }
            h.mirrored = false;
        }
        self.combo = None;
    }

    /// Brings the cursor back to the host at `at` (or where it left). `resync`: the cursor
    /// crossed an edge, so Shift/Ctrl still held are pressed on the host (injected): the
    /// host never saw them go down, and keyboard focus follows the cursor.
    fn return_home(&mut self, at: Option<Point>, resync: bool, out: &mut Vec<Action>) {
        let p = self.layout.host.clamp_nearest(at.unwrap_or(self.return_point));
        self.mode = Mode::Local;
        self.forget_remote();
        self.remote_buttons = 0;
        self.edge_since = None;
        self.last_local = p;
        out.push(Action::ShowCursor);
        out.push(Action::WarpCursor(p));
        out.push(Action::KeepAwake(false));
        if resync && self.settings.forward_keyboard {
            let skip = self.settings.edge_modifier.bit();
            for vk in keys::MODIFIER_VKS {
                let bit = keys::modifier_bit(vk);
                if bit & (mods::SHIFT | mods::CTRL) == 0 || bit == skip {
                    continue;
                }
                if let Some(h) = &mut self.keys[vk as usize]
                    && h.route != Route::Local
                {
                    h.route = Route::Local;
                    h.repeat = false;
                    out.push(Action::LocalKey { vk, scan: h.info.scan, extended: h.info.extended, down: true });
                }
            }
        }
        out.push(Action::ActiveChanged(None));
    }

    /// Switches to client `ci` without crossing an edge (hotkey, UI).
    fn jump_to(&mut self, ci: usize, out: &mut Vec<Action>) {
        let entry = self.layout.clients[ci].screen.monitors[0].center();
        match self.mode.clone() {
            Mode::Local => {
                self.return_point = self.last_local;
                self.enter(ci, entry, true, false, out);
            }
            Mode::Remote { id, .. } => {
                if id == self.layout.clients[ci].id {
                    return;
                }
                self.leave_client(&id, out);
                self.enter(ci, entry, false, false, out);
            }
        }
    }

    /// Keyboard switching: to the neighbour in `dir`, or home for `None`.
    fn switch_dir(&mut self, dir: Option<Edge>, out: &mut Vec<Action>) {
        let from = match &self.mode {
            Mode::Local => Target::Host,
            Mode::Remote { id, .. } => self.layout.client_index(id).map_or(Target::Host, Target::Client),
        };
        let to = match dir {
            None => Some(Target::Host),
            Some(d) => self.layout.neighbor(from, d),
        };
        match to {
            Some(Target::Host) => self.force_local(out),
            Some(Target::Client(ci)) => self.jump_to(ci, out),
            None => log::debug!("no computer {dir:?} of the current one"),
        }
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
                out.push(Action::Send { client: id.clone(), msg: Message::MouseButton { button, down } });
                if down {
                    self.remote_buttons |= bit;
                    // Ctrl+click, Win+click...: a shortcut performed on the client.
                    if self.combo.is_none() && self.held_mods() != 0 {
                        self.combo = Some(Route::Remote);
                    }
                    self.mask_host(out);
                } else {
                    self.remote_buttons &= !bit;
                }
                Verdict::Swallow
            }
        }
    }

    fn on_key(&mut self, vk: u16, info: KeyInfo, down: bool, now_ms: u64, out: &mut Vec<Action>) -> Verdict {
        let Some(slot) = self.keys.get(vk as usize).copied() else { return Verdict::Pass };
        if !down {
            return self.on_release(vk, info, out);
        }
        if let Some(mut h) = slot {
            if now_ms.saturating_sub(h.seen_ms) <= STALE_MS {
                // Auto-repeat goes where the press went.
                h.seen_ms = now_ms;
                self.keys[vk as usize] = Some(h);
                return match h.route {
                    Route::Local if h.repeat => Verdict::Pass,
                    Route::Remote => {
                        self.send(key_msg(vk, h.info, true), out);
                        Verdict::Swallow
                    }
                    _ => Verdict::Swallow,
                };
            }
            // Its release was missed: release it where it went and start over.
            if h.route == Route::Remote || h.mirrored {
                self.send(key_msg(vk, h.info, false), out);
            }
            self.keys[vk as usize] = None;
        }
        self.on_press(vk, info, now_ms, out)
    }

    fn on_press(&mut self, vk: u16, info: KeyInfo, now_ms: u64, out: &mut Vec<Action>) -> Verdict {
        let bit = keys::modifier_bit(vk);
        let held = self.held_mods();
        let mut entry = Held { route: Route::Local, info, mirrored: false, repeat: true, masked: false, seen_ms: now_ms };

        // Moush's own hotkeys work on the physical key state and never reach any computer.
        if bit == 0 && self.hotkey(vk, held, out) {
            entry.route = Route::Dropped;
            self.keys[vk as usize] = Some(entry);
            self.mask_host(out);
            return Verdict::Swallow;
        }

        if !(self.settings.forward_keyboard && matches!(self.mode, Mode::Remote { .. })) {
            // Typing goes to the host.
            if bit == 0 && held != 0 && self.combo.is_none() {
                self.combo = Some(Route::Local);
            }
            self.keys[vk as usize] = Some(entry);
            return Verdict::Pass;
        }

        let to_host = match self.combo {
            Some(route) => route == Route::Local,
            None if bit != 0 || held == 0 => false,
            None => {
                self.settings.local_shortcuts.matches(held, vk)
                    || (!self.settings.forward_system_shortcuts && keys::is_system_shortcut(held, vk))
            }
        };
        if bit == 0 && held != 0 && self.combo.is_none() {
            self.combo = Some(if to_host { Route::Local } else { Route::Remote });
        }
        if to_host {
            // The whole shortcut acts on the host: move its modifiers there first.
            let injected = self.modifiers_to_host(out);
            self.keys[vk as usize] = Some(entry);
            if injected {
                // Injected modifiers are queued behind the event being processed, so the
                // key itself is injected too to keep the order.
                out.push(Action::LocalKey { vk, scan: info.scan, extended: info.extended, down: true });
                return Verdict::Swallow;
            }
            return Verdict::Pass;
        }
        if bit == 0 && held != 0 {
            // A shortcut on the client: press every held modifier there too.
            self.modifiers_to_client(out);
        }
        entry.route = Route::Remote;
        self.keys[vk as usize] = Some(entry);
        self.send(key_msg(vk, info, true), out);
        self.mask_host(out);
        Verdict::Swallow
    }

    fn on_release(&mut self, vk: u16, info: KeyInfo, out: &mut Vec<Action>) -> Verdict {
        let Some(h) = self.keys[vk as usize].take() else {
            // A release without a press we saw. Forward it while typing goes to a client:
            // an extra release is harmless, a missing one leaves a stuck key.
            if self.settings.forward_keyboard && matches!(self.mode, Mode::Remote { .. }) {
                self.send(key_msg(vk, info, false), out);
                return Verdict::Swallow;
            }
            return Verdict::Pass;
        };
        let verdict = match h.route {
            Route::Local => {
                if h.mirrored {
                    self.send(key_msg(vk, h.info, false), out);
                }
                Verdict::Pass
            }
            Route::Remote => {
                let lone_win = keys::modifier_bit(vk) == mods::WIN && self.combo.is_none() && self.held_mods() == 0;
                if lone_win && !self.settings.forward_system_shortcuts {
                    // A Win tap is the Start menu shortcut: open it on the host, not the client.
                    self.release_on_client(vk, h.info, out);
                    out.push(Action::LocalKey { vk, scan: h.info.scan, extended: h.info.extended, down: true });
                    out.push(Action::LocalKey { vk, scan: h.info.scan, extended: h.info.extended, down: false });
                } else {
                    self.send(key_msg(vk, h.info, false), out);
                }
                Verdict::Swallow
            }
            Route::Dropped => Verdict::Swallow,
        };
        if keys::is_modifier(vk) && self.held_mods() == 0 {
            self.combo = None;
        }
        verdict
    }

    /// Ctrl+Alt+Esc (home), Scroll Lock (lock), Ctrl+Alt+Shift+Arrow/Home (switch).
    /// Returns true if `vk` was consumed.
    fn hotkey(&mut self, vk: u16, held: u8, out: &mut Vec<Action>) -> bool {
        if vk == keys::VK_SCROLL && self.settings.scroll_lock_toggle {
            self.locked = !self.locked;
            self.edge_since = None;
            out.push(Action::LockChanged(self.locked));
            return true;
        }
        if vk == keys::VK_ESCAPE && held & (mods::CTRL | mods::ALT) == mods::CTRL | mods::ALT && self.active().is_some() {
            self.force_local(out);
            return true;
        }
        if self.settings.switch_hotkeys && held == mods::CTRL | mods::ALT | mods::SHIFT {
            let dir = match vk {
                keys::VK_LEFT => Some(Edge::Left),
                keys::VK_RIGHT => Some(Edge::Right),
                keys::VK_UP => Some(Edge::Top),
                keys::VK_DOWN => Some(Edge::Bottom),
                keys::VK_HOME => None,
                _ => return false,
            };
            self.switch_dir(dir, out);
            return true;
        }
        false
    }

    /// Modifier families physically held (whatever their destination).
    fn held_mods(&self) -> u8 {
        keys::MODIFIER_VKS
            .iter()
            .filter(|vk| self.keys[**vk as usize].is_some())
            .fold(0, |m, vk| m | keys::modifier_bit(*vk))
    }

    /// Hands every held modifier to the host: released on the client if it was forwarded,
    /// injected on the host if the host never saw it. Returns true if anything was
    /// injected on the host.
    fn modifiers_to_host(&mut self, out: &mut Vec<Action>) -> bool {
        let mut injected = false;
        for vk in keys::MODIFIER_VKS {
            let Some(mut h) = self.keys[vk as usize] else { continue };
            match h.route {
                Route::Local => {
                    if h.mirrored {
                        self.release_on_client(vk, h.info, out);
                        h.mirrored = false;
                    }
                }
                Route::Remote | Route::Dropped => {
                    if h.route == Route::Remote {
                        self.release_on_client(vk, h.info, out);
                    }
                    out.push(Action::LocalKey { vk, scan: h.info.scan, extended: h.info.extended, down: true });
                    h.route = Route::Local;
                    h.masked = false;
                    injected = true;
                }
            }
            self.keys[vk as usize] = Some(h);
        }
        injected
    }

    /// Presses every held modifier on the active client that it does not have yet.
    fn modifiers_to_client(&mut self, out: &mut Vec<Action>) {
        for vk in keys::MODIFIER_VKS {
            let Some(mut h) = self.keys[vk as usize] else { continue };
            match h.route {
                Route::Local if !h.mirrored => h.mirrored = true,
                Route::Dropped => h.route = Route::Remote,
                _ => continue,
            }
            self.send(key_msg(vk, h.info, true), out);
            self.keys[vk as usize] = Some(h);
        }
    }

    /// Releases a forwarded key on the client; Win/Alt get the mask key first so the
    /// release does not count as a lone tap there.
    fn release_on_client(&self, vk: u16, info: KeyInfo, out: &mut Vec<Action>) {
        if keys::is_menu_modifier(vk) {
            self.send(Message::Key { vk: keys::VK_MASK, scan: 0, flags: key_flags::DOWN }, out);
            self.send(Message::Key { vk: keys::VK_MASK, scan: 0, flags: 0 }, out);
        }
        self.send(key_msg(vk, info, false), out);
    }

    /// Input between a host Win/Alt press and its release went elsewhere: inject the mask
    /// key on the host (once per press) so the release does not open the Start menu or
    /// activate a menu bar there.
    fn mask_host(&mut self, out: &mut Vec<Action>) {
        let mut need = false;
        for vk in [keys::VK_LWIN, keys::VK_RWIN, keys::VK_LMENU, keys::VK_RMENU, keys::VK_MENU] {
            if let Some(h) = &mut self.keys[vk as usize]
                && h.route == Route::Local
                && !h.masked
            {
                h.masked = true;
                need = true;
            }
        }
        if need {
            out.push(Action::LocalKey { vk: keys::VK_MASK, scan: 0, extended: false, down: true });
            out.push(Action::LocalKey { vk: keys::VK_MASK, scan: 0, extended: false, down: false });
        }
    }

    /// Sends to the active client, if any.
    fn send(&self, msg: Message, out: &mut Vec<Action>) {
        if let Mode::Remote { id, .. } = &self.mode {
            out.push(Action::Send { client: id.clone(), msg });
        }
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
mod tests;
