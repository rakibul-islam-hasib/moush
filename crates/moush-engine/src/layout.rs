//! Persistent client placements and construction of the router's [`Layout`].

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use moush_core::geometry::{ClientPlacement, Layout, Rect, RectF, Screen};
use moush_core::protocol::MonitorDesc;
use moush_core::util::write_atomic;

use crate::win::monitors::Monitor;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMonitor {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub dpi: u16,
    pub primary: bool,
    pub name: String,
}

impl From<&MonitorDesc> for SavedMonitor {
    fn from(m: &MonitorDesc) -> Self {
        Self { x: m.x, y: m.y, width: m.width, height: m.height, dpi: m.dpi, primary: m.primary, name: m.name.clone() }
    }
}

impl From<&SavedMonitor> for MonitorDesc {
    fn from(m: &SavedMonitor) -> Self {
        Self { x: m.x, y: m.y, width: m.width, height: m.height, dpi: m.dpi, primary: m.primary, name: m.name.clone() }
    }
}

/// A client the host has seen: where the user placed it and its last known monitors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedClient {
    pub id: String,
    pub name: String,
    /// Top-left in layout units; `None` until the user places it.
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub monitors: Vec<SavedMonitor>,
}

pub struct LayoutStore {
    path: Option<PathBuf>,
    pub clients: Vec<SavedClient>,
}

impl LayoutStore {
    pub fn load(path: Option<PathBuf>) -> Self {
        let clients = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| match serde_json::from_str::<Vec<SavedClient>>(&s) {
                Ok(v) => Some(v),
                Err(e) => {
                    log::warn!("ignoring unreadable layout file: {e}");
                    None
                }
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|c| !c.monitors.is_empty())
            .collect();
        Self { path, clients }
    }

    pub fn save(&self) -> io::Result<()> {
        let Some(p) = &self.path else { return Ok(()) };
        let json = serde_json::to_string_pretty(&self.clients).map_err(io::Error::other)?;
        write_atomic(p, json.as_bytes())
    }

    /// Records the monitors a client reported. Returns true if anything changed.
    pub fn remember(&mut self, id: &str, name: &str, monitors: &[MonitorDesc]) -> bool {
        let saved: Vec<SavedMonitor> = monitors.iter().map(SavedMonitor::from).collect();
        match self.clients.iter_mut().find(|c| c.id == id) {
            Some(c) if c.name == name && c.monitors == saved => false,
            Some(c) => {
                c.name = name.to_owned();
                c.monitors = saved;
                true
            }
            None => {
                self.clients.push(SavedClient { id: id.to_owned(), name: name.to_owned(), x: None, y: None, monitors: saved });
                true
            }
        }
    }

    pub fn set_position(&mut self, id: &str, x: f64, y: f64) -> bool {
        match self.clients.iter_mut().find(|c| c.id == id) {
            Some(c) => {
                c.x = Some(x);
                c.y = Some(y);
                true
            }
            None => false,
        }
    }

    pub fn forget(&mut self, id: &str) -> bool {
        let before = self.clients.len();
        self.clients.retain(|c| c.id != id);
        before != self.clients.len()
    }
}

/// A client rectangle for the layout editor.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientView {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub rect: RectF,
    pub resolution: (i32, i32),
    pub scale: f64,
    pub monitors: Vec<MonitorDesc>,
    pub adjacent: bool,
}

fn screen_of(monitors: &[MonitorDesc]) -> Screen {
    let mut rects: Vec<(bool, Rect)> = monitors.iter().map(|m| (m.primary, Rect::new(m.x, m.y, m.width, m.height))).collect();
    rects.sort_by_key(|(p, r)| (!*p, r.x, r.y));
    Screen::new(rects.into_iter().map(|(_, r)| r).collect())
}

fn primary_dpi(monitors: &[MonitorDesc]) -> f64 {
    monitors.iter().find(|m| m.primary).or(monitors.first()).map(|m| m.dpi as f64).unwrap_or(96.0)
}

/// Builds the router layout (online clients only) and the editor view (all known
/// clients). Clients without a saved position get a default one to the left of the
/// host's leftmost monitor, so a freshly paired laptop works immediately.
pub fn build(host: &[Monitor], online: &[String], store: &LayoutStore) -> (Layout, Vec<ClientView>) {
    let host_screen = Screen::new(if host.is_empty() {
        vec![Rect::new(0, 0, 1920, 1080)]
    } else {
        host.iter().map(|m| m.rect).collect()
    });
    let host_dpi = host.iter().find(|m| m.primary).or(host.first()).map(|m| m.dpi as f64).unwrap_or(96.0);
    let host_rects: Vec<RectF> = host_screen.monitors.iter().map(RectF::from_rect).collect();

    let mut placed: Vec<RectF> = Vec::new();
    let mut views = Vec::new();
    let mut clients = Vec::new();
    // Clients with a saved position first, so defaults avoid them.
    let mut order: Vec<&SavedClient> = store.clients.iter().collect();
    order.sort_by_key(|c| c.x.is_none());
    for c in order {
        let monitors: Vec<MonitorDesc> = c.monitors.iter().map(MonitorDesc::from).collect();
        let screen = screen_of(&monitors);
        let bbox = screen.bbox();
        let scale = host_dpi / primary_dpi(&monitors);
        let (w, h) = (bbox.w as f64 * scale, bbox.h as f64 * scale);
        let (x, y) = match (c.x, c.y) {
            (Some(x), Some(y)) => (x, y),
            _ => default_position(&host_screen, w, h, &placed),
        };
        let rect = RectF { x, y, w, h };
        placed.push(rect);
        let is_online = online.contains(&c.id);
        if is_online {
            clients.push(ClientPlacement { id: c.id.clone(), screen, origin_x: x, origin_y: y, scale });
        }
        views.push(ClientView {
            id: c.id.clone(),
            name: c.name.clone(),
            online: is_online,
            rect,
            resolution: (bbox.w, bbox.h),
            scale: primary_dpi(&monitors) / 96.0,
            monitors,
            adjacent: false,
        });
    }
    for i in 0..views.len() {
        let r = views[i].rect;
        views[i].adjacent = host_rects.iter().any(|h| r.touches(h))
            || views.iter().enumerate().any(|(j, o)| j != i && o.online && r.touches(&o.rect));
    }
    views.sort_by(|a, b| b.online.cmp(&a.online).then(a.name.cmp(&b.name)));
    (Layout { host: host_screen, clients }, views)
}

fn default_position(host: &Screen, w: f64, h: f64, placed: &[RectF]) -> (f64, f64) {
    let leftmost = *host.monitors.iter().min_by_key(|m| (m.x, m.y)).expect("host has monitors");
    let mut x = host.bbox().x as f64 - w;
    let y = leftmost.y as f64 + (leftmost.h as f64 - h) / 2.0;
    let y = y.round();
    loop {
        let r = RectF { x, y, w, h };
        if !placed.iter().any(|p| p.overlaps(&r)) {
            return (x.round(), y);
        }
        x -= w;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Vec<Monitor> {
        vec![
            Monitor { device: "1".into(), name: "M1".into(), rect: Rect::new(0, 0, 2560, 1440), dpi: 96, primary: true },
            Monitor { device: "2".into(), name: "M2".into(), rect: Rect::new(2560, 0, 2560, 1440), dpi: 96, primary: false },
        ]
    }

    fn laptop() -> Vec<MonitorDesc> {
        vec![MonitorDesc { x: 0, y: 0, width: 1920, height: 1200, dpi: 144, primary: true, name: "Built-in".into() }]
    }

    #[test]
    fn new_client_defaults_to_the_left_and_is_reachable() {
        let mut store = LayoutStore::load(None);
        store.remember("lap", "Laptop", &laptop());
        let (layout, views) = build(&host(), &["lap".into()], &store);
        assert_eq!(layout.clients.len(), 1);
        let v = &views[0];
        assert_eq!((v.rect.x, v.rect.w.round(), v.rect.h.round()), (-1280.0, 1280.0, 800.0));
        assert_eq!(v.rect.y, 320.0);
        assert!(v.adjacent);
        assert!(layout.host_edge_exit(moush_core::geometry::Point::new(0, 700), 0).is_some());
    }

    #[test]
    fn saved_position_is_used_and_offline_clients_are_not_routed() {
        let mut store = LayoutStore::load(None);
        store.remember("lap", "Laptop", &laptop());
        store.set_position("lap", 5120.0, 0.0);
        let (layout, views) = build(&host(), &[], &store);
        assert!(layout.clients.is_empty());
        assert!(!views[0].online);
        assert_eq!(views[0].rect.x, 5120.0);
        assert!(views[0].adjacent);
        store.set_position("lap", 9000.0, 0.0);
        let (_, views) = build(&host(), &["lap".into()], &store);
        assert!(!views[0].adjacent, "detached client is flagged");
    }
}
