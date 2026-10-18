//! Layout geometry: which screen edge leads where.
//!
//! There are two coordinate systems:
//!
//! * **Desktop pixels** – each computer's own Windows virtual desktop, in physical pixels.
//! * **Layout units** – one shared plane used by the layout editor. Layout units equal the
//!   host's physical pixels, so the host's monitors sit at their real Windows positions.
//!   A client is a rectangle placed by the user; its size in layout units is its desktop
//!   size normalised by DPI (`scale = host_dpi / client_dpi`), so a 150 % laptop appears at
//!   roughly its physical size next to 100 % desktop monitors.
//!
//! Crossing between two monitors of the same computer is left to Windows. Only an edge
//! that is *not* shared with another monitor of the same computer, and that touches
//! another computer's rectangle in the layout, transfers the cursor.

use serde::{Deserialize, Serialize};

/// A gap of up to this many layout units between rectangles still counts as touching.
pub const TOUCH_TOLERANCE: f64 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Integer rectangle; `right()` and `bottom()` are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }
    pub fn contains_f(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64 && x < self.right() as f64 && y >= self.y as f64 && y < self.bottom() as f64
    }
    /// Nearest pixel inside the rectangle.
    pub fn clamp(&self, p: Point) -> Point {
        Point::new(p.x.clamp(self.x, self.right() - 1), p.y.clamp(self.y, self.bottom() - 1))
    }
    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2, self.y + self.h / 2)
    }
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }
}

/// Floating-point rectangle in layout units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectF {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn from_rect(r: &Rect) -> Self {
        Self { x: r.x as f64, y: r.y as f64, w: r.w as f64, h: r.h as f64 }
    }
    /// Strict interior overlap (touching edges do not count).
    pub fn overlaps(&self, o: &RectF) -> bool {
        const EPS: f64 = 0.5;
        self.x + EPS < o.right() && o.x + EPS < self.right() && self.y + EPS < o.bottom() && o.y + EPS < self.bottom()
    }
    /// True when an edge of `self` lies within `TOUCH_TOLERANCE` of an edge of `o`
    /// and the two edges overlap by at least one unit.
    pub fn touches(&self, o: &RectF) -> bool {
        let t = TOUCH_TOLERANCE;
        let v_overlap = self.y.max(o.y) + 1.0 <= self.bottom().min(o.bottom());
        let h_overlap = self.x.max(o.x) + 1.0 <= self.right().min(o.right());
        (v_overlap && ((self.right() - o.x).abs() <= t || (o.right() - self.x).abs() <= t))
            || (h_overlap && ((self.bottom() - o.y).abs() <= t || (o.bottom() - self.y).abs() <= t))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

/// One computer's desktop: its monitors in its own desktop pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub monitors: Vec<Rect>,
}

impl Screen {
    pub fn new(monitors: Vec<Rect>) -> Self {
        assert!(!monitors.is_empty(), "a screen needs at least one monitor");
        Self { monitors }
    }

    pub fn bbox(&self) -> Rect {
        self.monitors.iter().skip(1).fold(self.monitors[0], |acc, m| acc.union(m))
    }

    pub fn contains(&self, p: Point) -> bool {
        self.monitors.iter().any(|m| m.contains(p))
    }

    pub fn contains_f(&self, x: f64, y: f64) -> bool {
        self.monitors.iter().any(|m| m.contains_f(x, y))
    }

    pub fn monitor_at(&self, p: Point) -> Option<&Rect> {
        self.monitors.iter().find(|m| m.contains(p))
    }

    /// Nearest pixel that lies on some monitor. Handles non-rectangular desktops.
    pub fn clamp_nearest(&self, p: Point) -> Point {
        self.monitors
            .iter()
            .map(|m| m.clamp(p))
            .min_by_key(|c| {
                let dx = (c.x - p.x) as i64;
                let dy = (c.y - p.y) as i64;
                dx * dx + dy * dy
            })
            .expect("screen has monitors")
    }
}

/// A client computer placed in the layout.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientPlacement {
    pub id: String,
    pub screen: Screen,
    /// Top-left of the client's desktop bounding box, in layout units.
    pub origin_x: f64,
    pub origin_y: f64,
    /// Layout units per client pixel.
    pub scale: f64,
}

impl ClientPlacement {
    pub fn layout_rect(&self) -> RectF {
        let b = self.screen.bbox();
        RectF { x: self.origin_x, y: self.origin_y, w: b.w as f64 * self.scale, h: b.h as f64 * self.scale }
    }

    /// Converts a layout position to the client pixel that contains it.
    pub fn from_layout(&self, lx: f64, ly: f64) -> Point {
        let b = self.screen.bbox();
        Point::new(
            b.x + ((lx - self.origin_x) / self.scale).floor() as i32,
            b.y + ((ly - self.origin_y) / self.scale).floor() as i32,
        )
    }

    /// Layout x of the left boundary of client pixel column `px`.
    fn layout_x(&self, px: f64) -> f64 {
        self.origin_x + (px - self.screen.bbox().x as f64) * self.scale
    }

    fn layout_y(&self, py: f64) -> f64 {
        self.origin_y + (py - self.screen.bbox().y as f64) * self.scale
    }
}

/// Which computer a crossing leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Host,
    Client(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crossing {
    pub edge: Edge,
    pub to: Target,
    /// Entry position in the destination computer's desktop pixels.
    pub point: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Stay(Point),
    Cross(Crossing),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub host: Screen,
    pub clients: Vec<ClientPlacement>,
}

impl Layout {
    pub fn client_index(&self, id: &str) -> Option<usize> {
        self.clients.iter().position(|c| c.id == id)
    }

    /// The host cursor is at `p` (Windows clamps it to the desktop, so pushing against an
    /// outer edge leaves it on the edge pixel). Returns a crossing when `p` sits on an edge
    /// pixel that leads to a client.
    pub fn host_edge_exit(&self, p: Point, corner_dead_zone: i32) -> Option<Crossing> {
        if self.clients.is_empty() {
            return None;
        }
        let m = *self.host.monitor_at(p)?;
        for edge in edges_at(&m, p) {
            if in_corner_dead_zone(&m, p, edge, corner_dead_zone) {
                continue;
            }
            let (px, py) = probe_outside(&m, p, edge);
            if self.host.contains_f(px, py) {
                continue; // boundary between two host monitors: Windows handles it
            }
            if let Some((ci, point)) = self.client_entry(None, edge, px, py) {
                return Some(Crossing { edge, to: Target::Client(ci), point });
            }
        }
        None
    }

    /// Moves the shared cursor on client `ci` from `p` by `(dx, dy)` client pixels.
    pub fn client_step(&self, ci: usize, p: Point, dx: i32, dy: i32, corner_dead_zone: i32) -> Step {
        let c = &self.clients[ci];
        let q = Point::new(p.x.saturating_add(dx), p.y.saturating_add(dy));
        if c.screen.contains(q) {
            return Step::Stay(q);
        }
        let m = match c.screen.monitor_at(p) {
            Some(m) => *m,
            None => *c.screen.monitor_at(c.screen.clamp_nearest(p)).expect("clamped point is on a monitor"),
        };
        // Exit edges ordered by how far the move overshoots them.
        let mut exits: Vec<(Edge, i32)> = Vec::with_capacity(2);
        if q.x < m.x {
            exits.push((Edge::Left, m.x - q.x));
        } else if q.x >= m.right() {
            exits.push((Edge::Right, q.x - m.right() + 1));
        }
        if q.y < m.y {
            exits.push((Edge::Top, m.y - q.y));
        } else if q.y >= m.bottom() {
            exits.push((Edge::Bottom, q.y - m.bottom() + 1));
        }
        exits.sort_by_key(|e| std::cmp::Reverse(e.1));

        for (edge, _) in exits {
            // The edge pixel where the cursor leaves.
            let e = match edge {
                Edge::Left => Point::new(m.x, q.y.clamp(m.y, m.bottom() - 1)),
                Edge::Right => Point::new(m.right() - 1, q.y.clamp(m.y, m.bottom() - 1)),
                Edge::Top => Point::new(q.x.clamp(m.x, m.right() - 1), m.y),
                Edge::Bottom => Point::new(q.x.clamp(m.x, m.right() - 1), m.bottom() - 1),
            };
            if in_corner_dead_zone(&m, e, edge, corner_dead_zone) {
                continue;
            }
            // Probe half a layout unit beyond the monitor edge, centred on the edge pixel.
            let half = 0.5;
            let (lx, ly) = match edge {
                Edge::Left => (c.layout_x(m.x as f64) - half, c.layout_y(e.y as f64 + 0.5)),
                Edge::Right => (c.layout_x(m.right() as f64) + half, c.layout_y(e.y as f64 + 0.5)),
                Edge::Top => (c.layout_x(e.x as f64 + 0.5), c.layout_y(m.y as f64) - half),
                Edge::Bottom => (c.layout_x(e.x as f64 + 0.5), c.layout_y(m.bottom() as f64) + half),
            };
            if c.screen.contains(c.from_layout(lx, ly)) {
                continue; // another monitor of this client
            }
            if let Some(point) = self.host_entry(edge, lx, ly) {
                return Step::Cross(Crossing { edge, to: Target::Host, point });
            }
            if let Some((cj, point)) = self.client_entry(Some(ci), edge, lx, ly) {
                return Step::Cross(Crossing { edge, to: Target::Client(cj), point });
            }
        }
        Step::Stay(c.screen.clamp_nearest(q))
    }

    /// Layout rectangle of a computer (the host's is the bounding box of its monitors).
    pub fn rect_of(&self, t: Target) -> Option<RectF> {
        match t {
            Target::Host => Some(RectF::from_rect(&self.host.bbox())),
            Target::Client(i) => self.clients.get(i).map(|c| c.layout_rect()),
        }
    }

    /// The computer next to `from` in direction `dir`, for keyboard switching.
    ///
    /// Computers lying beyond `from`'s edge in that direction (within the touch tolerance)
    /// come first; among them, one that overlaps `from` across the direction wins over a
    /// diagonal one, then the smallest gap wins. If nothing lies fully beyond the edge, a
    /// computer whose centre is mostly in that direction is accepted.
    pub fn neighbor(&self, from: Target, dir: Edge) -> Option<Target> {
        let a = self.rect_of(from)?;
        let (acx, acy) = (a.x + a.w / 2.0, a.y + a.h / 2.0);
        std::iter::once(Target::Host)
            .chain((0..self.clients.len()).map(Target::Client))
            .filter(|t| *t != from)
            .filter_map(|t| {
                let b = self.rect_of(t)?;
                let (bcx, bcy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
                let sy = span_gap(a.y, a.bottom(), b.y, b.bottom());
                let sx = span_gap(a.x, a.right(), b.x, b.right());
                // (gap beyond the edge, offset across, centre offset along, centre offset across, extends past the edge)
                let (gap, perp, along_c, perp_c, beyond) = match dir {
                    Edge::Left => (a.x - b.right(), sy, acx - bcx, acy - bcy, b.x < a.x),
                    Edge::Right => (b.x - a.right(), sy, bcx - acx, acy - bcy, b.right() > a.right()),
                    Edge::Top => (a.y - b.bottom(), sx, acy - bcy, acx - bcx, b.y < a.y),
                    Edge::Bottom => (b.y - a.bottom(), sx, bcy - acy, acx - bcx, b.bottom() > a.bottom()),
                };
                let tier = if gap >= -TOUCH_TOLERANCE {
                    0
                } else if beyond && along_c > 0.0 && along_c >= perp_c.abs() {
                    1
                } else {
                    return None;
                };
                Some((t, tier, perp, gap.max(0.0)))
            })
            .min_by(|x, y| (x.1, x.2, x.3).partial_cmp(&(y.1, y.2, y.3)).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(t, ..)| t)
    }

    /// Finds the host monitor reached by leaving through `edge` at layout point (lx, ly).
    /// The entry point is placed one pixel inside the monitor edge, so the cursor does not
    /// bounce straight back.
    fn host_entry(&self, edge: Edge, lx: f64, ly: f64) -> Option<Point> {
        let t = TOUCH_TOLERANCE;
        self.host.monitors.iter().find_map(|h| {
            let hf = RectF::from_rect(h);
            let hit = if edge.is_vertical() {
                ly >= hf.y && ly < hf.bottom() && lx >= hf.x - t && lx < hf.right() + t
            } else {
                lx >= hf.x && lx < hf.right() && ly >= hf.y - t && ly < hf.bottom() + t
            };
            if !hit {
                return None;
            }
            let inset = |lo: i32, hi: i32| if hi - lo > 2 { 1 } else { 0 };
            let x = (lx.floor() as i32).clamp(h.x, h.right() - 1);
            let y = (ly.floor() as i32).clamp(h.y, h.bottom() - 1);
            Some(match edge {
                // Leaving a client to the left enters the host monitor through its right edge.
                Edge::Left => Point::new(h.right() - 1 - inset(h.x, h.right()), y),
                Edge::Right => Point::new(h.x + inset(h.x, h.right()), y),
                Edge::Top => Point::new(x, h.bottom() - 1 - inset(h.y, h.bottom())),
                Edge::Bottom => Point::new(x, h.y + inset(h.y, h.bottom())),
            })
        })
    }

    /// Finds a client (other than `exclude`) reached through `edge` at (lx, ly) and the
    /// entry pixel on that client's matching edge.
    fn client_entry(&self, exclude: Option<usize>, edge: Edge, lx: f64, ly: f64) -> Option<(usize, Point)> {
        let t = TOUCH_TOLERANCE;
        self.clients.iter().enumerate().find_map(|(i, c)| {
            if Some(i) == exclude {
                return None;
            }
            let r = c.layout_rect();
            let hit = if edge.is_vertical() {
                ly >= r.y && ly < r.bottom() && lx >= r.x - t && lx < r.right() + t
            } else {
                lx >= r.x && lx < r.right() && ly >= r.y - t && ly < r.bottom() + t
            };
            if !hit {
                return None;
            }
            const EPS: f64 = 1e-6;
            let cx = lx.clamp(r.x, r.right() - EPS);
            let cy = ly.clamp(r.y, r.bottom() - EPS);
            let p = c.from_layout(cx, cy);
            // Snap to the client's far edge in the direction of travel.
            let b = c.screen.bbox();
            let p = match edge {
                Edge::Left => Point::new(b.right() - 1, p.y),
                Edge::Right => Point::new(b.x, p.y),
                Edge::Top => Point::new(p.x, b.bottom() - 1),
                Edge::Bottom => Point::new(p.x, b.y),
            };
            Some((i, c.screen.clamp_nearest(p)))
        })
    }
}

/// Distance between the intervals [a0, a1) and [b0, b1); 0 when they overlap.
fn span_gap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (b0 - a1).max(a0 - b1).max(0.0)
}

/// Edges of `m` that `p` lies on.
fn edges_at(m: &Rect, p: Point) -> impl Iterator<Item = Edge> {
    let mut v = [None; 4];
    if p.x == m.x {
        v[0] = Some(Edge::Left);
    }
    if p.x == m.right() - 1 {
        v[1] = Some(Edge::Right);
    }
    if p.y == m.y {
        v[2] = Some(Edge::Top);
    }
    if p.y == m.bottom() - 1 {
        v[3] = Some(Edge::Bottom);
    }
    v.into_iter().flatten()
}

/// Layout point half a unit outside `m`, next to edge pixel `p`.
fn probe_outside(m: &Rect, p: Point, edge: Edge) -> (f64, f64) {
    match edge {
        Edge::Left => (m.x as f64 - 0.5, p.y as f64 + 0.5),
        Edge::Right => (m.right() as f64 + 0.5, p.y as f64 + 0.5),
        Edge::Top => (p.x as f64 + 0.5, m.y as f64 - 0.5),
        Edge::Bottom => (p.x as f64 + 0.5, m.bottom() as f64 + 0.5),
    }
}

fn in_corner_dead_zone(m: &Rect, p: Point, edge: Edge, zone: i32) -> bool {
    if zone <= 0 {
        return false;
    }
    let (along, lo, hi) = if edge.is_vertical() { (p.y, m.y, m.bottom() - 1) } else { (p.x, m.x, m.right() - 1) };
    along - lo < zone || hi - along < zone
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference desk: [ Laptop ] [ Monitor 1 ] [ Monitor 2 ].
    /// Two 2560x1440 monitors at 100 %, a 1920x1200 laptop at 150 % to the left.
    fn desk() -> Layout {
        Layout {
            host: Screen::new(vec![Rect::new(0, 0, 2560, 1440), Rect::new(2560, 0, 2560, 1440)]),
            clients: vec![ClientPlacement {
                id: "laptop".into(),
                screen: Screen::new(vec![Rect::new(0, 0, 1920, 1200)]),
                origin_x: -1280.0,
                origin_y: 320.0,
                scale: 96.0 / 144.0,
            }],
        }
    }

    #[test]
    fn layout_rect_is_dpi_normalised() {
        let r = desk().clients[0].layout_rect();
        assert_eq!((r.x, r.y, r.w.round(), r.h.round()), (-1280.0, 320.0, 1280.0, 800.0));
    }

    #[test]
    fn left_edge_of_monitor_one_enters_laptop_right_edge() {
        let c = desk().host_edge_exit(Point::new(0, 700), 0).expect("crossing");
        assert_eq!(c.edge, Edge::Left);
        assert_eq!(c.to, Target::Client(0));
        assert_eq!(c.point.x, 1919);
        // (700.5 - 320) / (2/3) = 570.75
        assert_eq!(c.point.y, 570);
    }

    #[test]
    fn boundary_between_host_monitors_stays_local() {
        let l = desk();
        assert_eq!(l.host_edge_exit(Point::new(2559, 700), 0), None);
        assert_eq!(l.host_edge_exit(Point::new(2560, 700), 0), None);
    }

    #[test]
    fn edges_without_a_neighbour_do_nothing() {
        let l = desk();
        assert_eq!(l.host_edge_exit(Point::new(5119, 700), 0), None); // right of monitor 2
        assert_eq!(l.host_edge_exit(Point::new(1000, 0), 0), None); // top
        assert_eq!(l.host_edge_exit(Point::new(0, 100), 0), None); // left, above the laptop
        assert_eq!(l.host_edge_exit(Point::new(0, 1300), 0), None); // left, below the laptop
        assert_eq!(l.host_edge_exit(Point::new(5, 700), 0), None); // not on an edge
    }

    #[test]
    fn corner_dead_zone_blocks_switching_near_corners() {
        let mut l = desk();
        l.clients[0].origin_y = 0.0; // laptop flush with the top of monitor 1
        assert!(l.host_edge_exit(Point::new(0, 10), 0).is_some());
        assert!(l.host_edge_exit(Point::new(0, 10), 50).is_none());
        assert!(l.host_edge_exit(Point::new(0, 60), 50).is_some());
    }

    #[test]
    fn laptop_moves_and_returns_to_monitor_one() {
        let l = desk();
        assert_eq!(l.client_step(0, Point::new(1000, 500), 10, -5, 0), Step::Stay(Point::new(1010, 495)));
        // Pushing left on the laptop clamps (nothing there).
        assert_eq!(l.client_step(0, Point::new(2, 500), -10, 0, 0), Step::Stay(Point::new(0, 500)));
        // Pushing right crosses back into monitor 1, one pixel inside its left edge.
        let Step::Cross(c) = l.client_step(0, Point::new(1919, 570), 3, 0, 0) else { panic!("expected crossing") };
        assert_eq!(c.to, Target::Host);
        assert_eq!(c.edge, Edge::Right);
        assert_eq!(c.point, Point::new(1, 700));
    }

    #[test]
    fn round_trip_is_stable() {
        let l = desk();
        for y in [330, 500, 900, 1100] {
            let into = l.host_edge_exit(Point::new(0, y), 0).unwrap();
            let Step::Cross(back) = l.client_step(0, into.point, 1, 0, 0) else { panic!() };
            assert!((back.point.y - y).abs() <= 1, "y {y} came back as {}", back.point.y);
        }
    }

    #[test]
    fn laptop_to_the_right_of_monitor_two() {
        let mut l = desk();
        l.clients[0].origin_x = 5120.0;
        let c = l.host_edge_exit(Point::new(5119, 700), 0).unwrap();
        assert_eq!(c.point.x, 0);
        let Step::Cross(back) = l.client_step(0, Point::new(0, c.point.y), -1, 0, 0) else { panic!() };
        assert_eq!(back.point.x, 5118);
        assert_eq!(l.host_edge_exit(Point::new(0, 700), 0), None);
    }

    #[test]
    fn small_gap_is_tolerated_but_large_gap_is_not() {
        let mut l = desk();
        l.clients[0].origin_x = -1281.5;
        assert!(l.host_edge_exit(Point::new(0, 700), 0).is_some());
        l.clients[0].origin_x = -1300.0;
        assert!(l.host_edge_exit(Point::new(0, 700), 0).is_none());
    }

    #[test]
    fn client_with_two_monitors_moves_internally() {
        let mut l = desk();
        l.clients[0].screen = Screen::new(vec![Rect::new(0, 0, 1920, 1200), Rect::new(1920, 0, 1280, 1024)]);
        l.clients[0].scale = 0.5;
        // Crossing between the client's monitors stays on the client.
        assert_eq!(l.client_step(0, Point::new(1919, 100), 5, 0, 0), Step::Stay(Point::new(1924, 100)));
        // Below the shorter monitor there is no desktop: clamp to the nearest pixel.
        assert_eq!(l.client_step(0, Point::new(2000, 1020), 0, 50, 0), Step::Stay(Point::new(2000, 1023)));
    }

    #[test]
    fn client_to_client_crossing() {
        let mut l = desk();
        l.clients.push(ClientPlacement {
            id: "tablet".into(),
            screen: Screen::new(vec![Rect::new(0, 0, 1000, 800)]),
            origin_x: -2280.0,
            origin_y: 320.0,
            scale: 1.0,
        });
        let Step::Cross(c) = l.client_step(0, Point::new(0, 100), -1, 0, 0) else { panic!() };
        assert_eq!(c.to, Target::Client(1));
        assert_eq!(c.point.x, 999);
    }

    #[test]
    fn keyboard_neighbours_follow_the_layout() {
        // [ tablet ] [ laptop ] [ Monitor 1 ] [ Monitor 2 ], plus a TV above monitor 2.
        let mut l = desk();
        l.clients.push(ClientPlacement {
            id: "tablet".into(),
            screen: Screen::new(vec![Rect::new(0, 0, 1000, 800)]),
            origin_x: -2280.0,
            origin_y: 320.0,
            scale: 1.0,
        });
        l.clients.push(ClientPlacement {
            id: "tv".into(),
            screen: Screen::new(vec![Rect::new(0, 0, 1920, 1080)]),
            origin_x: 2900.0,
            origin_y: -1080.0,
            scale: 1.0,
        });
        assert_eq!(l.neighbor(Target::Host, Edge::Left), Some(Target::Client(0)));
        assert_eq!(l.neighbor(Target::Client(0), Edge::Left), Some(Target::Client(1)), "client to client");
        assert_eq!(l.neighbor(Target::Client(1), Edge::Right), Some(Target::Client(0)));
        assert_eq!(l.neighbor(Target::Client(0), Edge::Right), Some(Target::Host), "client to host");
        assert_eq!(l.neighbor(Target::Host, Edge::Top), Some(Target::Client(2)));
        assert_eq!(l.neighbor(Target::Client(2), Edge::Bottom), Some(Target::Host));
        assert_eq!(l.neighbor(Target::Host, Edge::Right), None);
        assert_eq!(l.neighbor(Target::Client(1), Edge::Left), None);
        assert_eq!(l.neighbor(Target::Client(9), Edge::Left), None);
    }

    #[test]
    fn keyboard_neighbour_prefers_overlap_over_diagonal() {
        let mut l = desk();
        // A second client far left but level with the host, and the laptop moved up-left diagonally.
        l.clients[0].origin_y = -1000.0;
        l.clients.push(ClientPlacement {
            id: "far".into(),
            screen: Screen::new(vec![Rect::new(0, 0, 1000, 800)]),
            origin_x: -4000.0,
            origin_y: 200.0,
            scale: 1.0,
        });
        assert_eq!(l.neighbor(Target::Host, Edge::Left), Some(Target::Client(1)));
    }

    #[test]
    fn touches_and_overlaps() {
        let a = RectF { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        assert!(a.touches(&RectF { x: 100.0, y: 50.0, w: 10.0, h: 10.0 }));
        assert!(!a.touches(&RectF { x: 110.0, y: 50.0, w: 10.0, h: 10.0 }));
        assert!(!a.touches(&RectF { x: 100.0, y: 100.0, w: 10.0, h: 10.0 })); // corner only
        assert!(a.overlaps(&RectF { x: 50.0, y: 50.0, w: 100.0, h: 100.0 }));
        assert!(!a.overlaps(&RectF { x: 100.0, y: 0.0, w: 100.0, h: 100.0 }));
    }
}
