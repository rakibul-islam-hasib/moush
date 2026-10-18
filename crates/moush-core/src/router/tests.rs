use super::*;
use crate::geometry::{ClientPlacement, Rect, Screen};

const LAPTOP: &str = "laptop";
const TABLET: &str = "tablet";
const PARK: Point = Point::new(1280, 720);

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

/// [ tablet ] [ laptop ] [ Monitor 1 ] [ Monitor 2 ]
fn desk_with_tablet() -> Layout {
    let mut l = desk();
    l.clients.push(ClientPlacement {
        id: TABLET.into(),
        screen: Screen::new(vec![Rect::new(0, 0, 1000, 800)]),
        origin_x: -2920.0,
        origin_y: 0.0,
        scale: 1.0,
    });
    l
}

fn router() -> Router {
    Router::new(desk(), RouterSettings::default())
}

fn router_with(s: RouterSettings) -> Router {
    Router::new(desk(), s)
}

fn sent(out: &[Action]) -> Vec<Message> {
    out.iter()
        .filter_map(|a| match a {
            Action::Send { msg, .. } => Some(msg.clone()),
            _ => None,
        })
        .collect()
}

fn sent_to(out: &[Action], who: &str) -> Vec<Message> {
    out.iter()
        .filter_map(|a| match a {
            Action::Send { client, msg } if client == who => Some(msg.clone()),
            _ => None,
        })
        .collect()
}

/// Keys injected on the host: (vk, down).
fn injected(out: &[Action]) -> Vec<(u16, bool)> {
    out.iter()
        .filter_map(|a| match a {
            Action::LocalKey { vk, down, .. } => Some((*vk, *down)),
            _ => None,
        })
        .collect()
}

fn kd(vk: u16) -> Message {
    Message::Key { vk, scan: vk, flags: key_flags::DOWN }
}

fn ku(vk: u16) -> Message {
    Message::Key { vk, scan: vk, flags: 0 }
}

fn mask() -> Vec<Message> {
    vec![Message::Key { vk: keys::VK_MASK, scan: 0, flags: key_flags::DOWN }, Message::Key { vk: keys::VK_MASK, scan: 0, flags: 0 }]
}

/// Pushes the cursor against the left edge of monitor 1 at height `y`.
fn cross_left(r: &mut Router, y: i32) -> Vec<Action> {
    let mut out = Vec::new();
    let v = r.handle(RawInput::MouseMove { x: 0, y }, Point::new(1, y), 0, &mut out);
    assert_eq!(v, Verdict::Swallow);
    out
}

/// Moves the shared cursor by (dx, dy) host pixels while a client is active.
fn nudge(r: &mut Router, dx: i32, dy: i32) -> Vec<Action> {
    let mut out = Vec::new();
    r.handle(RawInput::MouseMove { x: PARK.x + dx, y: PARK.y + dy }, PARK, 0, &mut out);
    out
}

fn key_at(r: &mut Router, vk: u16, down: bool, t: u64) -> (Verdict, Vec<Action>) {
    let mut out = Vec::new();
    let v = r.handle(RawInput::Key { vk, scan: vk, extended: false, down }, Point::new(1280, 720), t, &mut out);
    (v, out)
}

fn key(r: &mut Router, vk: u16, down: bool) -> (Verdict, Vec<Message>) {
    let (v, out) = key_at(r, vk, down, 0);
    (v, sent(&out))
}

fn click(r: &mut Router, down: bool) -> Vec<Action> {
    let mut out = Vec::new();
    r.handle(RawInput::MouseButton { button: MouseButton::Left, down }, PARK, 0, &mut out);
    out
}

// ---------------------------------------------------------------- mouse

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
    assert_eq!(r.handle(RawInput::MouseMove { x: 1270, y: 725 }, PARK, 0, &mut out), Verdict::Swallow);
    assert_eq!(sent(&out), vec![Message::MouseMove { x: 1909, y: 605 }]);
}

#[test]
fn warp_artefacts_are_ignored() {
    let mut r = router();
    cross_left(&mut r, 600);
    let mut out = Vec::new();
    // A stale event from the edge position, delivered after the park warp.
    r.handle(RawInput::MouseMove { x: 0, y: 601 }, PARK, 0, &mut out);
    assert!(sent(&out).is_empty());
}

#[test]
fn moving_back_returns_to_host() {
    let mut r = router();
    cross_left(&mut r, 600);
    let out = nudge(&mut r, 10, 0);
    assert_eq!(r.active(), None);
    assert_eq!(sent(&out), vec![Message::Leave]);
    assert!(out.contains(&Action::ShowCursor));
    assert!(out.contains(&Action::WarpCursor(Point::new(1, 600))));
    assert!(out.contains(&Action::ActiveChanged(None)));
    assert!(injected(&out).is_empty());
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
    let mut out = click(&mut r, true);
    // Pushing right at the laptop's edge would normally return to the host.
    out.extend(nudge(&mut r, 10, 0));
    assert_eq!(r.active(), Some(LAPTOP));
    assert_eq!(sent(&out), vec![Message::MouseButton { button: MouseButton::Left, down: true }]);
}

#[test]
fn buttons_and_wheel_are_forwarded() {
    let mut r = router();
    cross_left(&mut r, 600);
    let mut out = Vec::new();
    r.handle(RawInput::MouseButton { button: MouseButton::Right, down: true }, PARK, 0, &mut out);
    r.handle(RawInput::MouseButton { button: MouseButton::Right, down: false }, PARK, 0, &mut out);
    r.handle(RawInput::Wheel { axis: WheelAxis::Vertical, delta: -120 }, PARK, 0, &mut out);
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
fn scroll_lock_locks_the_cursor() {
    let mut r = router();
    let (v, _) = key(&mut r, keys::VK_SCROLL, true);
    assert_eq!(v, Verdict::Swallow);
    assert_eq!(key(&mut r, keys::VK_SCROLL, false).0, Verdict::Swallow);
    assert!(r.locked());
    let mut out = Vec::new();
    assert_eq!(r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(1, 600), 0, &mut out), Verdict::Pass);
    assert_eq!(r.active(), None);
}

#[test]
fn switch_delay_requires_resting_on_the_edge() {
    let mut r = router_with(RouterSettings { switch_delay_ms: 150, ..Default::default() });
    let mut out = Vec::new();
    let ev = RawInput::MouseMove { x: 0, y: 600 };
    assert_eq!(r.handle(ev, Point::new(0, 600), 1000, &mut out), Verdict::Pass);
    assert_eq!(r.handle(ev, Point::new(0, 600), 1100, &mut out), Verdict::Pass);
    assert_eq!(r.handle(ev, Point::new(0, 600), 1160, &mut out), Verdict::Swallow);
    assert_eq!(r.active(), Some(LAPTOP));
}

#[test]
fn pointer_speed_accumulates_fractions() {
    let mut layout = desk();
    layout.clients[0].scale = 2.0; // a 50 % client: half a client pixel per host pixel
    let mut r = Router::new(layout, RouterSettings::default());
    let mut out = Vec::new();
    r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(1, 600), 0, &mut out);
    out.clear();
    r.handle(RawInput::MouseMove { x: 1279, y: 720 }, PARK, 0, &mut out);
    assert!(sent(&out).is_empty(), "half a pixel does not move yet");
    r.handle(RawInput::MouseMove { x: 1279, y: 720 }, PARK, 0, &mut out);
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

// ---------------------------------------------------------------- basic keyboard routing

#[test]
fn keys_follow_the_active_computer() {
    let mut r = router();
    assert_eq!(key(&mut r, 0x41, true).0, Verdict::Pass);
    assert_eq!(key(&mut r, 0x41, false).0, Verdict::Pass);
    cross_left(&mut r, 600);
    assert_eq!(key(&mut r, 0x41, true), (Verdict::Swallow, vec![kd(0x41)]));
    assert_eq!(key(&mut r, 0x41, false), (Verdict::Swallow, vec![ku(0x41)]));
}

#[test]
fn remote_auto_repeat_is_forwarded() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, 0x41, true);
    assert_eq!(key(&mut r, 0x41, true), (Verdict::Swallow, vec![kd(0x41)]));
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
    assert_eq!(sent(&out), vec![Message::Enter { x: 1919, y: 600 }, kd(keys::VK_LSHIFT)]);
    // Releasing it releases it on both computers.
    assert_eq!(key(&mut r, keys::VK_LSHIFT, false), (Verdict::Pass, vec![ku(keys::VK_LSHIFT)]));
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
    // Esc never reaches anyone; Ctrl/Alt were pressed (and released by Leave) on the
    // client only, so the host does not see their releases either.
    assert_eq!(key(&mut r, keys::VK_ESCAPE, false), (Verdict::Swallow, vec![]));
    assert_eq!(key(&mut r, keys::VK_LMENU, false), (Verdict::Swallow, vec![]));
    assert_eq!(key(&mut r, keys::VK_LCONTROL, false), (Verdict::Swallow, vec![]));
    // Afterwards the keyboard is plainly local again.
    assert_eq!(key(&mut r, keys::VK_LMENU, true), (Verdict::Pass, vec![]));
}

// ---------------------------------------------------------------- mouse-only mode

#[test]
fn mouse_only_mode_keeps_typing_on_the_host() {
    let mut r = router_with(RouterSettings { forward_keyboard: false, ..Default::default() });
    key(&mut r, keys::VK_LSHIFT, true);
    let out = cross_left(&mut r, 600);
    assert_eq!(sent(&out), vec![Message::Enter { x: 1919, y: 600 }], "no modifier mirrored");
    assert_eq!(key(&mut r, 0x41, true), (Verdict::Pass, vec![]));
    assert_eq!(key(&mut r, 0x41, true), (Verdict::Pass, vec![]), "auto-repeat stays on the host");
    assert_eq!(key(&mut r, 0x41, false), (Verdict::Pass, vec![]));
    assert_eq!(key(&mut r, keys::VK_LSHIFT, false), (Verdict::Pass, vec![]));
    // Moush's hotkeys still work.
    key(&mut r, keys::VK_LCONTROL, true);
    key(&mut r, keys::VK_LMENU, true);
    assert_eq!(key(&mut r, keys::VK_ESCAPE, true).0, Verdict::Swallow);
    assert_eq!(r.active(), None);
}

#[test]
fn changing_keyboard_forwarding_while_keys_are_held_never_strands_a_key() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, 0x41, true); // forwarded
    r.set_settings(RouterSettings { forward_keyboard: false, ..Default::default() });
    // The release follows the press to the client, not the new setting.
    assert_eq!(key(&mut r, 0x41, false), (Verdict::Swallow, vec![ku(0x41)]));
    assert_eq!(key(&mut r, 0x42, true), (Verdict::Pass, vec![])); // now local
    r.set_settings(RouterSettings::default());
    assert_eq!(key(&mut r, 0x42, false), (Verdict::Pass, vec![]), "released where pressed");
    assert_eq!(key(&mut r, 0x43, true), (Verdict::Swallow, vec![kd(0x43)]));
}

#[test]
fn host_alt_is_masked_when_a_click_goes_to_the_client() {
    let mut r = router_with(RouterSettings { forward_keyboard: false, ..Default::default() });
    cross_left(&mut r, 600);
    assert_eq!(key(&mut r, keys::VK_LMENU, true).0, Verdict::Pass);
    let out = click(&mut r, true);
    assert_eq!(sent(&out), vec![Message::MouseButton { button: MouseButton::Left, down: true }]);
    assert_eq!(injected(&out), vec![(keys::VK_MASK, true), (keys::VK_MASK, false)]);
    assert!(injected(&click(&mut r, false)).is_empty());
    assert!(injected(&click(&mut r, true)).is_empty(), "masked once per press");
    assert_eq!(key(&mut r, keys::VK_LMENU, false).0, Verdict::Pass);
}

// ---------------------------------------------------------------- system shortcuts

#[test]
fn alt_tab_goes_to_the_client_by_default() {
    let mut r = router();
    cross_left(&mut r, 600);
    assert_eq!(key(&mut r, keys::VK_LMENU, true), (Verdict::Swallow, vec![kd(keys::VK_LMENU)]));
    assert_eq!(key(&mut r, keys::VK_TAB, true), (Verdict::Swallow, vec![kd(keys::VK_TAB)]));
    assert_eq!(key(&mut r, keys::VK_TAB, false), (Verdict::Swallow, vec![ku(keys::VK_TAB)]));
    assert_eq!(key(&mut r, keys::VK_LMENU, false), (Verdict::Swallow, vec![ku(keys::VK_LMENU)]));
}

#[test]
fn system_shortcut_is_handed_to_the_host_atomically() {
    let mut r = router_with(RouterSettings { forward_system_shortcuts: false, ..Default::default() });
    cross_left(&mut r, 600);
    // Alt alone could be the start of Alt+F (menu) on the client: it is forwarded.
    assert_eq!(key(&mut r, keys::VK_LMENU, true), (Verdict::Swallow, vec![kd(keys::VK_LMENU)]));
    // Tab makes it Alt+Tab: Alt is released on the client (masked, so no menu bar) and
    // pressed on the host before Tab, which is re-injected to keep the order.
    let (v, out) = key_at(&mut r, keys::VK_TAB, true, 0);
    assert_eq!(v, Verdict::Swallow);
    let mut expect = mask();
    expect.push(ku(keys::VK_LMENU));
    assert_eq!(sent(&out), expect);
    assert_eq!(injected(&out), vec![(keys::VK_LMENU, true), (keys::VK_TAB, true)]);
    assert_eq!(key(&mut r, keys::VK_TAB, false), (Verdict::Pass, vec![]));
    // The rest of the shortcut stays on the host: Shift for Alt+Shift+Tab, more Tabs.
    assert_eq!(key(&mut r, keys::VK_LSHIFT, true), (Verdict::Pass, vec![]));
    let (v, out) = key_at(&mut r, keys::VK_TAB, true, 0);
    assert_eq!((v, out), (Verdict::Pass, vec![]));
    key(&mut r, keys::VK_TAB, false);
    key(&mut r, keys::VK_LSHIFT, false);
    assert_eq!(key(&mut r, keys::VK_LMENU, false), (Verdict::Pass, vec![]));
    // Shortcut over: typing goes to the client again.
    assert_eq!(key(&mut r, 0x41, true), (Verdict::Swallow, vec![kd(0x41)]));
}

#[test]
fn app_shortcuts_still_reach_the_client_when_system_shortcuts_stay_local() {
    let mut r = router_with(RouterSettings { forward_system_shortcuts: false, ..Default::default() });
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LCONTROL, true);
    assert_eq!(key(&mut r, 0x43, true), (Verdict::Swallow, vec![kd(0x43)]));
    key(&mut r, 0x43, false);
    key(&mut r, keys::VK_LCONTROL, false);
    key(&mut r, keys::VK_LMENU, true);
    assert_eq!(key(&mut r, 0x46, true), (Verdict::Swallow, vec![kd(0x46)]), "Alt+F");
}

#[test]
fn win_shortcut_and_ctrl_shift_esc_stay_local_when_not_forwarded() {
    let mut r = router_with(RouterSettings { forward_system_shortcuts: false, ..Default::default() });
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LWIN, true);
    let (v, out) = key_at(&mut r, 0x45, true, 0);
    assert_eq!(v, Verdict::Swallow);
    let mut expect = mask();
    expect.push(ku(keys::VK_LWIN));
    assert_eq!(sent(&out), expect, "Win released on the client without opening Start");
    assert_eq!(injected(&out), vec![(keys::VK_LWIN, true), (0x45, true)]);
    key(&mut r, 0x45, false);
    assert_eq!(key(&mut r, keys::VK_LWIN, false), (Verdict::Pass, vec![]));

    key(&mut r, keys::VK_LCONTROL, true);
    key(&mut r, keys::VK_LSHIFT, true);
    let (_, out) = key_at(&mut r, keys::VK_ESCAPE, true, 0);
    assert_eq!(sent(&out), vec![ku(keys::VK_LSHIFT), ku(keys::VK_LCONTROL)]);
    assert_eq!(injected(&out), vec![(keys::VK_LSHIFT, true), (keys::VK_LCONTROL, true), (keys::VK_ESCAPE, true)]);
}

#[test]
fn lone_win_tap_opens_start_where_system_shortcuts_go() {
    // Default: the client's Start menu.
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LWIN, true);
    let (v, out) = key_at(&mut r, keys::VK_LWIN, false, 0);
    assert_eq!((v, sent(&out), injected(&out)), (Verdict::Swallow, vec![ku(keys::VK_LWIN)], vec![]));

    // Not forwarded: the host's Start menu, and nothing on the client.
    let mut r = router_with(RouterSettings { forward_system_shortcuts: false, ..Default::default() });
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LWIN, true);
    let (v, out) = key_at(&mut r, keys::VK_LWIN, false, 0);
    let mut expect = mask();
    expect.push(ku(keys::VK_LWIN));
    assert_eq!(v, Verdict::Swallow);
    assert_eq!(sent(&out), expect);
    assert_eq!(injected(&out), vec![(keys::VK_LWIN, true), (keys::VK_LWIN, false)]);

    // Win+click on the client is not a tap.
    key(&mut r, keys::VK_LWIN, true);
    click(&mut r, true);
    click(&mut r, false);
    let (_, out) = key_at(&mut r, keys::VK_LWIN, false, 0);
    assert_eq!((sent(&out), injected(&out)), (vec![ku(keys::VK_LWIN)], vec![]));
}

#[test]
fn host_win_released_after_typing_on_the_client_does_not_open_start() {
    let mut r = router();
    assert_eq!(key(&mut r, keys::VK_LWIN, true).0, Verdict::Pass);
    let out = cross_left(&mut r, 600);
    assert_eq!(sent(&out), vec![Message::Enter { x: 1919, y: 600 }], "Win is not mirrored by crossing");
    assert_eq!(injected(&out), vec![(keys::VK_MASK, true), (keys::VK_MASK, false)]);
    // Win+E typed on the client: Win joins the shortcut there.
    assert_eq!(key(&mut r, 0x45, true), (Verdict::Swallow, vec![kd(keys::VK_LWIN), kd(0x45)]));
    key(&mut r, 0x45, false);
    assert_eq!(key(&mut r, keys::VK_LWIN, false), (Verdict::Pass, vec![ku(keys::VK_LWIN)]));
}

// ---------------------------------------------------------------- local shortcuts

#[test]
fn local_shortcut_always_acts_on_the_host() {
    let (set, errors) = ShortcutSet::parse(&["ctrl+shift+esc", "nonsense"]);
    assert_eq!(errors.len(), 1);
    let mut r = router_with(RouterSettings { local_shortcuts: set, ..Default::default() });
    cross_left(&mut r, 600);
    assert_eq!(key(&mut r, keys::VK_LCONTROL, true).1, vec![kd(keys::VK_LCONTROL)]);
    assert_eq!(key(&mut r, keys::VK_LSHIFT, true).1, vec![kd(keys::VK_LSHIFT)]);
    let (v, out) = key_at(&mut r, keys::VK_ESCAPE, true, 0);
    assert_eq!(v, Verdict::Swallow);
    assert_eq!(sent(&out), vec![ku(keys::VK_LSHIFT), ku(keys::VK_LCONTROL)]);
    assert_eq!(injected(&out), vec![(keys::VK_LSHIFT, true), (keys::VK_LCONTROL, true), (keys::VK_ESCAPE, true)]);
    for vk in [keys::VK_ESCAPE, keys::VK_LSHIFT, keys::VK_LCONTROL] {
        assert_eq!(key(&mut r, vk, false), (Verdict::Pass, vec![]));
    }
    // Ctrl+Esc (a different shortcut) still goes to the client.
    key(&mut r, keys::VK_LCONTROL, true);
    assert_eq!(key(&mut r, keys::VK_ESCAPE, true), (Verdict::Swallow, vec![kd(keys::VK_ESCAPE)]));
}

#[test]
fn local_shortcut_typed_on_the_host_just_passes() {
    let (set, _) = ShortcutSet::parse(&["Ctrl+Shift+Esc"]);
    let mut r = router_with(RouterSettings { local_shortcuts: set, ..Default::default() });
    key(&mut r, keys::VK_LCONTROL, true);
    key(&mut r, keys::VK_LSHIFT, true);
    let (v, out) = key_at(&mut r, keys::VK_ESCAPE, true, 0);
    assert_eq!((v, out), (Verdict::Pass, vec![]));
}

// ---------------------------------------------------------------- switching hotkeys

fn hotkey_mods(r: &mut Router, down: bool) -> Vec<(Verdict, Vec<Message>)> {
    [keys::VK_LCONTROL, keys::VK_LMENU, keys::VK_LSHIFT].into_iter().map(|vk| key(r, vk, down)).collect()
}

#[test]
fn switch_hotkeys_walk_the_layout() {
    let mut r = Router::new(desk_with_tablet(), RouterSettings::default());
    let mut out = Vec::new();
    r.handle(RawInput::MouseMove { x: 700, y: 300 }, Point::new(701, 300), 0, &mut out);
    hotkey_mods(&mut r, true);

    let (v, out) = key_at(&mut r, keys::VK_LEFT, true, 0);
    assert_eq!(v, Verdict::Swallow);
    assert_eq!(r.active(), Some(LAPTOP));
    assert_eq!(sent(&out), vec![Message::Enter { x: 960, y: 600 }], "no modifier mirrored by a hotkey");
    assert_eq!(injected(&out), vec![(keys::VK_MASK, true), (keys::VK_MASK, false)], "host Alt masked");
    assert_eq!(key(&mut r, keys::VK_LEFT, false), (Verdict::Swallow, vec![]));

    // Client to client, then back, then home.
    let (_, out) = key_at(&mut r, keys::VK_LEFT, true, 0);
    assert_eq!(r.active(), Some(TABLET));
    assert_eq!(sent_to(&out, LAPTOP), vec![Message::Leave]);
    assert_eq!(sent_to(&out, TABLET), vec![Message::Enter { x: 500, y: 400 }]);
    key(&mut r, keys::VK_LEFT, false);
    assert_eq!(key(&mut r, keys::VK_LEFT, true), (Verdict::Swallow, vec![]), "nothing further left");
    key(&mut r, keys::VK_LEFT, false);
    key(&mut r, keys::VK_RIGHT, true);
    assert_eq!(r.active(), Some(LAPTOP));
    key(&mut r, keys::VK_RIGHT, false);
    let (_, out) = key_at(&mut r, keys::VK_HOME, true, 0);
    assert_eq!(r.active(), None);
    assert_eq!(sent(&out), vec![Message::Leave]);
    assert!(out.contains(&Action::WarpCursor(Point::new(700, 300))), "back where the cursor left the host");
    assert_eq!(key(&mut r, keys::VK_HOME, false), (Verdict::Swallow, vec![]));
    // The modifiers were pressed on the host and are released there.
    assert!(hotkey_mods(&mut r, false).iter().all(|(v, m)| *v == Verdict::Pass && m.is_empty()));
}

#[test]
fn switch_hotkey_from_a_client_back_to_the_host() {
    let mut r = router();
    cross_left(&mut r, 600);
    hotkey_mods(&mut r, true); // forwarded to the laptop
    let (v, m) = key(&mut r, keys::VK_RIGHT, true);
    assert_eq!((v, m), (Verdict::Swallow, vec![Message::Leave]));
    assert_eq!(r.active(), None);
    // Released on the laptop by Leave; the host never saw them.
    assert!(hotkey_mods(&mut r, false).iter().all(|(v, m)| *v == Verdict::Swallow && m.is_empty()));
}

#[test]
fn switch_hotkeys_can_be_disabled() {
    let mut r = router_with(RouterSettings { switch_hotkeys: false, ..Default::default() });
    cross_left(&mut r, 600);
    hotkey_mods(&mut r, true);
    assert_eq!(key(&mut r, keys::VK_RIGHT, true), (Verdict::Swallow, vec![kd(keys::VK_RIGHT)]));
    assert_eq!(r.active(), Some(LAPTOP));
}

// ---------------------------------------------------------------- edge rules

#[test]
fn edge_switching_can_require_a_modifier() {
    let mut r = router_with(RouterSettings { edge_modifier: EdgeModifier::Ctrl, ..Default::default() });
    let mut out = Vec::new();
    assert_eq!(r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(1, 600), 0, &mut out), Verdict::Pass);
    assert_eq!(r.active(), None);
    key(&mut r, keys::VK_LCONTROL, true);
    let out = cross_left(&mut r, 600);
    assert_eq!(sent(&out), vec![Message::Enter { x: 1919, y: 600 }], "the switch modifier is not mirrored");
    assert_eq!(key(&mut r, keys::VK_LCONTROL, false), (Verdict::Pass, vec![]));
    // Without Ctrl the laptop's edge holds the cursor.
    nudge(&mut r, 10, 0);
    assert_eq!(r.active(), Some(LAPTOP));
    // The keyboard hotkey and the UI still switch.
    let mut out = Vec::new();
    r.force_local(&mut out);
    assert!(r.switch_to(LAPTOP, Point::new(10, 10), &mut out));
}

#[test]
fn alt_tab_held_on_the_host_keeps_the_cursor_there() {
    let mut r = router();
    key(&mut r, keys::VK_LMENU, true);
    key(&mut r, keys::VK_TAB, true);
    key(&mut r, keys::VK_TAB, false);
    let mut out = Vec::new();
    assert_eq!(r.handle(RawInput::MouseMove { x: 0, y: 600 }, Point::new(1, 600), 0, &mut out), Verdict::Pass);
    assert_eq!(r.active(), None, "the Alt+Tab switcher is open on the host");
    key(&mut r, keys::VK_LMENU, false);
    cross_left(&mut r, 600);
    assert_eq!(r.active(), Some(LAPTOP));
}

#[test]
fn alt_tab_held_on_a_client_keeps_the_cursor_there() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LMENU, true);
    key(&mut r, keys::VK_TAB, true);
    nudge(&mut r, 10, 0);
    assert_eq!(r.active(), Some(LAPTOP));
    key(&mut r, keys::VK_TAB, false);
    key(&mut r, keys::VK_LMENU, false);
    nudge(&mut r, 10, 0);
    assert_eq!(r.active(), None);
}

#[test]
fn holding_alt_alone_does_not_block_crossing() {
    let mut r = router();
    key(&mut r, keys::VK_LMENU, true);
    let out = cross_left(&mut r, 600);
    assert_eq!(r.active(), Some(LAPTOP));
    assert_eq!(injected(&out), vec![(keys::VK_MASK, true), (keys::VK_MASK, false)], "no menu bar on the host");
}

// ---------------------------------------------------------------- focus follows the cursor

#[test]
fn shift_held_on_a_client_is_pressed_on_the_host_when_the_cursor_returns() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LSHIFT, true);
    let out = nudge(&mut r, 10, 0);
    assert_eq!(r.active(), None);
    assert_eq!(sent(&out), vec![Message::Leave]);
    assert_eq!(injected(&out), vec![(keys::VK_LSHIFT, true)]);
    assert_eq!(key(&mut r, keys::VK_LSHIFT, true), (Verdict::Swallow, vec![]), "no repeat after the switch");
    assert_eq!(key(&mut r, keys::VK_LSHIFT, false), (Verdict::Pass, vec![]));
}

#[test]
fn alt_and_win_held_on_a_client_are_not_re_pressed_on_the_host() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LWIN, true);
    key(&mut r, keys::VK_LMENU, true);
    let out = nudge(&mut r, 10, 0);
    assert_eq!(r.active(), None);
    assert!(injected(&out).is_empty());
    assert_eq!(key(&mut r, keys::VK_LWIN, false), (Verdict::Swallow, vec![]));
    assert_eq!(key(&mut r, keys::VK_LMENU, false), (Verdict::Swallow, vec![]));
}

#[test]
fn shift_follows_the_cursor_from_client_to_client() {
    let mut r = Router::new(desk_with_tablet(), RouterSettings::default());
    let mut out = Vec::new();
    r.switch_to(LAPTOP, Point::new(10, 10), &mut out);
    key(&mut r, keys::VK_LSHIFT, true);
    let mut out = Vec::new();
    for _ in 0..4 {
        out.extend(nudge(&mut r, -400, 0));
    }
    assert_eq!(r.active(), Some(TABLET));
    assert_eq!(sent_to(&out, LAPTOP).last(), Some(&Message::Leave));
    assert_eq!(sent_to(&out, TABLET)[..2], [Message::Enter { x: 999, y: 600 }, kd(keys::VK_LSHIFT)]);
    let (v, out) = key_at(&mut r, keys::VK_LSHIFT, false, 0);
    assert_eq!((v, sent_to(&out, TABLET)), (Verdict::Swallow, vec![ku(keys::VK_LSHIFT)]));
}

#[test]
fn a_test_switch_does_not_mirror_modifiers() {
    let mut r = router();
    key(&mut r, keys::VK_LSHIFT, true);
    let mut out = Vec::new();
    r.switch_to(LAPTOP, Point::new(10, 10), &mut out);
    assert_eq!(sent(&out), vec![Message::Enter { x: 960, y: 600 }]);
    assert_eq!(key(&mut r, keys::VK_LSHIFT, false), (Verdict::Pass, vec![]));
}

// ---------------------------------------------------------------- stuck-key prevention

#[test]
fn disconnect_while_active_brings_cursor_home() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, 0x41, true);
    let mut out = Vec::new();
    r.client_gone(LAPTOP, &mut out);
    assert_eq!(r.active(), None);
    assert!(sent(&out).is_empty());
    assert!(out.contains(&Action::WarpCursor(Point::new(0, 600))));
    assert!(out.contains(&Action::ShowCursor));
    // The host never saw A go down, so it does not see it go up either.
    assert_eq!(key(&mut r, 0x41, false), (Verdict::Swallow, vec![]));
}

#[test]
fn client_removed_from_the_layout_is_released() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LCONTROL, true);
    let mut out = Vec::new();
    r.set_layout(Layout { clients: vec![], ..desk() }, &mut out);
    assert_eq!(r.active(), None);
    assert_eq!(sent(&out), vec![Message::Leave]);
}

#[test]
fn session_lock_forgets_keys_the_secure_desktop_swallowed() {
    let mut r = router();
    cross_left(&mut r, 600);
    key(&mut r, keys::VK_LWIN, true);
    key(&mut r, 0x4C, true); // Win+L: the host locks no matter what
    let mut out = Vec::new();
    r.session_changed(&mut out);
    assert_eq!(sent(&out), vec![Message::Leave]);
    assert_eq!(r.active(), None);
    // The releases happened on the secure desktop. After unlocking, the keys work
    // normally on the host instead of being treated as still held.
    r.session_changed(&mut Vec::new());
    assert_eq!(key_at(&mut r, 0x4C, true, 100).0, Verdict::Pass);
    assert_eq!(key(&mut r, 0x4C, false).0, Verdict::Pass);
    assert_eq!(key(&mut r, keys::VK_LWIN, true).0, Verdict::Pass);
}

#[test]
fn resync_after_hook_gap_releases_forwarded_keys() {
    let mut r = router();
    key(&mut r, keys::VK_LSHIFT, true); // host key, mirrored on crossing
    key(&mut r, keys::VK_LCONTROL, true); // host key, released during the gap
    cross_left(&mut r, 600);
    key(&mut r, 0x41, true); // forwarded
    let mut out = Vec::new();
    r.resync_after_gap(|vk| vk == keys::VK_LSHIFT, &mut out);
    assert_eq!(sent(&out), vec![Message::ReleaseAll]);
    // Shift is still down on the host and only there.
    assert_eq!(key(&mut r, keys::VK_LSHIFT, false), (Verdict::Pass, vec![]));
    // A and Ctrl are forgotten: a fresh A goes to the client.
    assert_eq!(key(&mut r, 0x41, true), (Verdict::Swallow, vec![kd(0x41)]));
}

#[test]
fn a_repeat_long_after_the_press_is_a_new_press() {
    let mut r = router();
    key_at(&mut r, 0x41, true, 0);
    cross_left(&mut r, 600);
    assert_eq!(key_at(&mut r, 0x41, true, 100).0, Verdict::Swallow);
    assert_eq!(key_at(&mut r, 0x41, true, 4000).0, Verdict::Swallow, "repeats keep it alive");
    // 10 s without a repeat: its release was missed; this is a new press on the client.
    let (v, out) = key_at(&mut r, 0x41, true, 14_000);
    assert_eq!((v, sent(&out)), (Verdict::Swallow, vec![kd(0x41)]));
    assert_eq!(key(&mut r, 0x41, false), (Verdict::Swallow, vec![ku(0x41)]));
}

#[test]
fn stale_forwarded_key_is_released_before_its_new_press() {
    let mut r = router();
    cross_left(&mut r, 600);
    key_at(&mut r, 0x41, true, 0);
    let (_, out) = key_at(&mut r, 0x41, true, 10_000);
    assert_eq!(sent(&out), vec![ku(0x41), kd(0x41)]);
}

#[test]
fn unknown_release_while_remote_is_forwarded() {
    let mut r = router();
    cross_left(&mut r, 600);
    assert_eq!(key(&mut r, 0x41, false), (Verdict::Swallow, vec![ku(0x41)]));
}

#[test]
fn every_press_gets_exactly_one_release_somewhere() {
    // A long pseudo-random session of keys, mouse moves across the edge, hotkeys and
    // setting changes. Invariant: per computer, presses and releases of each key balance
    // once everything is released (the client also releases on Leave).
    let mut r = Router::new(desk_with_tablet(), RouterSettings::default());
    let vks = [0x41, 0x42, keys::VK_TAB, keys::VK_ESCAPE, keys::VK_LSHIFT, keys::VK_LCONTROL, keys::VK_LMENU, keys::VK_LWIN, keys::VK_LEFT];
    let mut host = [0i32; 256];
    let mut clients: std::collections::HashMap<String, [i32; 256]> = Default::default();
    let mut physical = [false; 256];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let apply = |out: &[Action], host: &mut [i32; 256], clients: &mut std::collections::HashMap<String, [i32; 256]>| {
        for a in out {
            match a {
                Action::LocalKey { vk, down, .. } => host[*vk as usize] = i32::from(*down),
                Action::Send { client, msg } => {
                    let c = clients.entry(client.clone()).or_insert([0; 256]);
                    match msg {
                        Message::Key { vk, flags, .. } => {
                            // Like InjectState: a release only counts if the key is down.
                            if flags & key_flags::DOWN != 0 {
                                c[*vk as usize] = 1;
                            } else {
                                c[*vk as usize] = 0;
                            }
                        }
                        Message::Leave | Message::ReleaseAll => *c = [0; 256],
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    };
    let mut checks = 0;
    for step in 0..20_000u64 {
        // A key that is not physically held is down on no computer.
        for vk in 0..256 {
            if !physical[vk] {
                checks += 1;
                assert_eq!(host[vk], 0, "step {step}: vk {vk:#x} stuck on the host");
                for (id, c) in &clients {
                    assert_eq!(c[vk], 0, "step {step}: vk {vk:#x} stuck on {id}");
                }
            }
        }
        let mut out = Vec::new();
        match rnd(10) {
            0..=5 => {
                let vk = vks[rnd(vks.len() as u64) as usize];
                let down = !physical[vk as usize] || rnd(3) == 0; // sometimes auto-repeat
                if !down || !physical[vk as usize] {
                    physical[vk as usize] = down;
                }
                let input = RawInput::Key { vk, scan: vk, extended: false, down };
                if r.handle(input, PARK, step, &mut out) == Verdict::Pass {
                    host[vk as usize] = if down { 1 } else { 0 };
                }
            }
            6 | 7 => {
                let cursor = if r.active().is_some() { PARK } else { Point::new(1 + rnd(2) as i32, 600) };
                let dx = rnd(801) as i32 - 400;
                let target = if r.active().is_some() { Point::new(PARK.x + dx, PARK.y) } else { Point::new(0, 600) };
                r.handle(RawInput::MouseMove { x: target.x, y: target.y }, cursor, step, &mut out);
            }
            8 => {
                let s = RouterSettings {
                    forward_keyboard: rnd(4) != 0,
                    forward_system_shortcuts: rnd(2) == 0,
                    local_shortcuts: ShortcutSet::parse(&["Ctrl+Shift+Esc", "Alt+B"]).0,
                    ..Default::default()
                };
                r.set_settings(s);
            }
            _ => {
                if rnd(20) == 0 {
                    r.session_changed(&mut out);
                    physical = [false; 256]; // released on the secure desktop
                    host = [0; 256];
                } else {
                    r.force_local(&mut out);
                }
            }
        }
        apply(&out, &mut host, &mut clients);
    }
    // Release everything still physically held.
    for vk in vks {
        if physical[vk as usize] {
            let mut out = Vec::new();
            if r.handle(RawInput::Key { vk, scan: vk, extended: false, down: false }, PARK, 30_000, &mut out) == Verdict::Pass {
                host[vk as usize] = 0;
            }
            apply(&out, &mut host, &mut clients);
        }
    }
    assert!(host.iter().all(|k| *k == 0), "key stuck on the host: {:?}", host.iter().position(|k| *k != 0));
    for (id, c) in &clients {
        assert!(c.iter().all(|k| *k == 0), "key {:?} stuck on {id}", c.iter().position(|k| *k != 0));
    }
    assert!(checks > 500, "the invariant was checked only {checks} times");
}
