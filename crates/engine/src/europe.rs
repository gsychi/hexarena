//! Mainland Europe, traced by hand in longitude/latitude and laid onto the hex board.
//! Britain, Scandinavia and the Mediterranean islands are left out so all the land is
//! one connected mass; the eastern edge runs off the board through Ukraine and Belarus.

use crate::mapgen::{
    affordable_expansions, has_two_independent_routes, keep_largest_component, open_within,
    passable, planar, GeneratedMap,
};
use crate::rng::{fbm, splitmix, Rng};
use crate::Board;
use hexarena_protocol::{Hex, MatchSettings, Terrain, TerrainKind};
use std::collections::VecDeque;
use std::sync::OnceLock;

/// Mainland coast, counter-clockwise from Cape St. Vincent. Points east of
/// [`OFF_BOARD_LON`] are an inland cut that lies past the board edge, and the Baltic
/// north of Lithuania may run off the board's corner too (see [`fits`]).
const COAST: &[(f64, f64)] = &[
    (-8.9, 37.0), (-7.9, 37.0), (-7.4, 37.2), (-6.4, 36.8), (-6.2, 36.3), (-5.6, 36.0),
    (-4.4, 36.7), (-2.9, 36.7), (-2.2, 36.7), (-1.6, 37.3), (-0.7, 37.6), (-0.5, 38.3),
    (0.2, 38.75), (-0.3, 39.4), (0.0, 40.0), (0.9, 40.7), (1.2, 41.1), (2.2, 41.4),
    (3.2, 41.9), (3.2, 42.4), (3.0, 42.9), (3.5, 43.3), (4.6, 43.4), (5.4, 43.2),
    (6.0, 43.1), (6.9, 43.5), (7.6, 43.8), (8.4, 44.2), (8.9, 44.4), (9.8, 44.1),
    (10.3, 43.5), (10.5, 42.9), (11.1, 42.4), (11.8, 42.0), (12.4, 41.6), (13.0, 41.25),
    (13.8, 41.2), (14.2, 40.8), (14.4, 40.6), (15.0, 40.3), (15.6, 40.0), (15.8, 39.4),
    (16.1, 38.7), (15.7, 38.2), (15.9, 37.95), (16.5, 38.4), (17.1, 38.9), (16.6, 39.6),
    (17.2, 40.45), (17.9, 40.3), (18.4, 39.8), (18.5, 40.15), (18.0, 40.65), (16.9, 41.1),
    (16.1, 41.4), (16.2, 41.9), (15.2, 41.9), (14.2, 42.5), (13.6, 43.6), (12.6, 44.0),
    (12.3, 44.5), (12.4, 45.0), (12.4, 45.45), (13.1, 45.65), (13.75, 45.65), (13.6, 45.1),
    (13.9, 44.8), (14.3, 45.25), (14.9, 44.6), (15.9, 43.7), (16.45, 43.5), (17.4, 43.0),
    (18.1, 42.65), (18.7, 42.3), (19.1, 42.0), (19.5, 41.8), (19.45, 41.3), (19.4, 40.5),
    (19.8, 40.05), (20.2, 39.6), (20.7, 39.0), (21.1, 38.4), (21.3, 37.7), (21.6, 37.2),
    (21.7, 36.8), (22.1, 36.9), (22.5, 36.4), (22.8, 36.8), (23.2, 36.45), (22.9, 37.5),
    (23.15, 37.3), (23.0, 37.9), (23.6, 37.95), (24.0, 37.65), (24.0, 38.2), (23.4, 38.4),
    (22.8, 38.9), (23.0, 39.2), (22.6, 40.0), (22.9, 40.6), (23.7, 40.2), (23.9, 40.6),
    (24.4, 40.9), (25.4, 40.9), (26.0, 40.8), (26.3, 40.1), (26.7, 40.4), (27.5, 40.97),
    (28.6, 41.0), (29.05, 41.1), (29.0, 41.25), (28.0, 41.6), (27.7, 42.0), (27.7, 42.5),
    (27.9, 43.2), (28.6, 43.7), (28.7, 44.2), (29.6, 44.85), (29.7, 45.4), (30.2, 45.85),
    (30.7, 46.4), (31.5, 46.6), (40.0, 46.6), (40.0, 59.0), (28.0, 59.4), (24.8, 59.45),
    (23.5, 59.2), (23.4, 58.6), (24.4, 58.3), (24.3, 57.2), (24.1, 57.0), (23.3, 57.0),
    (22.6, 57.75), (21.6, 57.4), (21.05, 56.8), (21.05, 56.0), (21.1, 55.7), (21.2, 55.2),
    (20.5, 54.95), (19.9, 54.4), (18.9, 54.35), (18.6, 54.4), (18.5, 54.75), (17.6, 54.8),
    (16.5, 54.55), (15.5, 54.2), (14.6, 53.95), (13.8, 54.15), (13.4, 54.5), (12.5, 54.45),
    (11.9, 54.15), (11.0, 53.95), (10.9, 54.35), (10.15, 54.45), (10.0, 54.75), (9.6, 55.0),
    (9.8, 55.5), (10.2, 55.85), (10.6, 56.25), (10.3, 56.55), (10.4, 57.1), (10.6, 57.75),
    (9.9, 57.55), (9.0, 57.15), (8.25, 56.75), (8.1, 56.0), (8.1, 55.5), (8.6, 55.0),
    (8.9, 54.4), (8.9, 53.9), (8.2, 53.55), (7.2, 53.4), (6.1, 53.45), (4.8, 53.0),
    (4.6, 52.45), (4.15, 52.0), (3.6, 51.5), (3.0, 51.25), (2.5, 51.08), (1.6, 50.95),
    (1.6, 50.2), (1.1, 49.95), (0.2, 49.7), (-0.3, 49.3), (-1.3, 49.4), (-1.9, 49.7),
    (-1.6, 48.7), (-2.7, 48.55), (-3.6, 48.75), (-4.7, 48.45), (-4.4, 48.0), (-4.1, 47.8),
    (-3.0, 47.5), (-2.4, 47.25), (-2.1, 46.8), (-1.4, 46.2), (-1.2, 45.6), (-1.2, 44.6),
    (-1.45, 43.55), (-2.5, 43.35), (-3.8, 43.45), (-5.7, 43.55), (-7.0, 43.55), (-8.3, 43.55),
    (-8.4, 43.35), (-9.25, 42.9), (-8.85, 42.25), (-8.85, 41.6), (-8.65, 41.15), (-8.85, 40.15),
    (-9.35, 39.35), (-9.5, 38.75), (-9.2, 38.65), (-8.9, 38.5), (-8.8, 38.0), (-8.8, 37.4),
];

const OFF_BOARD_LON: f64 = 35.0;

/// Coast points the board must hold. The Latvian and Estonian coast sits in the
/// board's pinched north-east corner, so it may be cut by the edge like the east.
fn fits(&(lon, lat): &(f64, f64)) -> bool {
    lon < OFF_BOARD_LON && !(lon > 20.5 && lat > 55.5)
}

/// Mountain chains as ridge lines.
const RANGES: &[&[(f64, f64)]] = &[
    // Pyrenees
    &[(-1.8, 43.1), (0.0, 42.75), (1.5, 42.55), (2.8, 42.45)],
    // Cantabrian
    &[(-7.2, 42.9), (-5.5, 43.0), (-4.0, 43.05)],
    // Sistema Central
    &[(-7.2, 40.3), (-5.5, 40.3), (-3.8, 40.85)],
    // Baetic / Sierra Nevada
    &[(-5.2, 36.8), (-3.4, 37.05), (-2.2, 37.4)],
    // Alps
    &[(5.8, 44.2), (6.8, 45.0), (7.0, 45.9), (8.0, 46.3), (9.3, 46.4), (10.5, 46.6), (12.0, 46.8), (13.5, 47.2), (15.5, 47.5)],
    // Jura
    &[(6.2, 46.6), (7.4, 47.3)],
    // Apennines
    &[(8.9, 44.5), (10.2, 44.3), (11.5, 43.8), (12.9, 42.9), (13.8, 41.9), (15.0, 41.0), (15.8, 40.3), (16.2, 39.3)],
    // Dinaric Alps and Pindus
    &[(15.0, 45.1), (16.3, 44.2), (17.8, 43.5), (19.3, 42.6), (20.3, 41.6), (21.0, 40.3), (21.6, 39.3), (22.0, 38.6)],
    // Balkan Mountains
    &[(22.4, 43.5), (24.0, 42.9), (26.0, 42.8), (27.3, 42.7)],
    // Rhodopes
    &[(23.2, 41.9), (24.6, 41.6)],
    // Carpathians
    &[(17.6, 48.8), (19.2, 49.35), (21.0, 49.3), (22.7, 49.0), (24.3, 48.1), (25.3, 47.3), (25.8, 46.2), (25.0, 45.45), (23.5, 45.4), (22.4, 45.3), (22.2, 44.7)],
    // Ore Mountains and Sudetes
    &[(12.5, 50.5), (14.5, 50.8), (16.5, 50.3)],
];

/// Upland blobs: centre and radius in degrees.
const UPLANDS: &[((f64, f64), f64)] = &[
    ((2.8, 45.3), 1.4),   // Massif Central
    ((9.5, 51.0), 1.3),   // German Central Uplands
    ((10.6, 51.8), 0.6),  // Harz
    ((15.5, 49.4), 1.0),  // Bohemian-Moravian Highlands
    ((5.5, 50.2), 0.8),   // Ardennes
    ((7.6, 48.2), 0.8),   // Vosges and Black Forest
    ((-3.5, 39.8), 2.6),  // Meseta
    ((20.5, 43.4), 1.2),  // Serbian highlands
    ((24.8, 46.8), 1.0),  // Transylvanian plateau
];

/// Cities used as start positions and map labels.
pub const PLACES: &[(&str, (f64, f64))] = &[
    ("Paris", (2.35, 48.86)), ("Madrid", (-3.7, 40.42)), ("Berlin", (13.4, 52.52)),
    ("Rome", (12.5, 41.9)), ("Warsaw", (21.0, 52.23)), ("Vienna", (16.37, 48.21)),
    ("Budapest", (19.04, 47.5)), ("Bucharest", (26.1, 44.43)), ("Athens", (23.73, 37.98)),
    ("Kyiv", (30.52, 50.45)), ("Amsterdam", (4.9, 52.37)), ("Prague", (14.42, 50.08)),
    ("Belgrade", (20.46, 44.8)), ("Lisbon", (-9.14, 38.72)), ("Sofia", (23.32, 42.7)),
    ("Riga", (24.1, 56.95)), ("Vilnius", (25.28, 54.69)), ("Minsk", (27.56, 53.9)),
    ("Milan", (9.19, 45.46)), ("Munich", (11.58, 48.14)), ("Lyon", (4.83, 45.76)),
    ("Barcelona", (2.17, 41.39)), ("Hamburg", (9.99, 53.55)), ("Bordeaux", (-0.58, 44.84)),
    ("Brussels", (4.35, 50.85)), ("Zagreb", (15.98, 45.81)), ("Istanbul", (28.97, 41.05)),
    ("Naples", (14.27, 40.85)), ("Seville", (-5.98, 37.39)), ("Krakow", (19.94, 50.06)),
];

const COS_LAT: f64 = 0.669; // cos 48°: keeps east-west distances true in mid-Europe

/// Degrees to board units and back.
#[derive(Clone, Copy)]
struct Projection {
    lon0: f64,
    lat0: f64,
    k: f64,
}

/// `max(|q|, |r|, |s|)` of a point in planar board units.
fn hex_norm(x: f64, y: f64) -> f64 {
    let r = y / 0.866_025_4;
    let q = x - r / 2.0;
    q.abs().max(r.abs()).max((q + r).abs())
}

impl Projection {
    fn fit(radius: i32) -> Projection {
        let (w, lon0, lat0) = *CENTRE.get_or_init(centre);
        Projection { lon0, lat0, k: (radius as f64 - 1.2) / w }
    }

    fn to_board(&self, (lon, lat): (f64, f64)) -> (f64, f64) {
        ((lon - self.lon0) * COS_LAT * self.k, (self.lat0 - lat) * self.k)
    }

    fn to_geo(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.lon0 + x / (self.k * COS_LAT), self.lat0 - y / self.k)
    }

    fn hex_of(&self, p: (f64, f64)) -> Hex {
        let (x, y) = self.to_board(p);
        let rf = y / 0.866_025_4;
        let qf = x - rf / 2.0;
        let sf = -qf - rf;
        let (mut q, mut r, s) = (qf.round(), rf.round(), sf.round());
        let (dq, dr, ds) = ((q - qf).abs(), (r - rf).abs(), (s - sf).abs());
        if dq > dr && dq > ds {
            q = -r - s;
        } else if dr > ds {
            r = -q - s;
        }
        Hex::new(q as i32, r as i32)
    }
}

static CENTRE: OnceLock<(f64, f64, f64)> = OnceLock::new();

/// The map centre (and the farthest coast point's distance at one board unit per
/// degree) where the board's slanted edges pinch the corners least.
fn centre() -> (f64, f64, f64) {
    let fit: Vec<(f64, f64)> = COAST.iter().copied().filter(fits).collect();
    let worst = |lon0: f64, lat0: f64| {
        let unit = Projection { lon0, lat0, k: 1.0 };
        fit.iter()
            .map(|&p| {
                let (x, y) = unit.to_board(p);
                hex_norm(x, y)
            })
            .fold(0.0, f64::max)
    };
    let mut best = (f64::MAX, 0.0, 0.0);
    for i in 0..=60 {
        for j in 0..=40 {
            let (lon0, lat0) = (i as f64 * 0.4, 42.0 + j as f64 * 0.25);
            let w = worst(lon0, lat0);
            if w < best.0 {
                best = (w, lon0, lat0);
            }
        }
    }
    best
}

fn on_land((lon, lat): (f64, f64)) -> bool {
    let mut inside = false;
    let mut j = COAST.len() - 1;
    for i in 0..COAST.len() {
        let (xi, yi) = COAST[i];
        let (xj, yj) = COAST[j];
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Distance in (east-west corrected) degrees from a point to a polyline.
fn to_line(p: (f64, f64), line: &[(f64, f64)]) -> f64 {
    let px = p.0 * COS_LAT;
    let mut best = f64::MAX;
    for w in line.windows(2) {
        let (ax, ay) = (w[0].0 * COS_LAT, w[0].1);
        let (bx, by) = (w[1].0 * COS_LAT, w[1].1);
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (p.1 - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (cx, cy) = (ax + t * dx, ay + t * dy);
        best = best.min(((px - cx).powi(2) + (p.1 - cy).powi(2)).sqrt());
    }
    best
}

fn ridge(p: (f64, f64)) -> f64 {
    RANGES
        .iter()
        .map(|l| (-(to_line(p, l) / 0.75).powi(2)).exp())
        .fold(0.0, f64::max)
}

fn upland(p: (f64, f64)) -> f64 {
    UPLANDS
        .iter()
        .map(|&(c, r)| {
            let d = (((p.0 - c.0) * COS_LAT).powi(2) + (p.1 - c.1).powi(2)).sqrt();
            (-(d / r).powi(2)).exp()
        })
        .fold(0.0, f64::max)
}

fn land_hexes(board: &Board, proj: &Projection) -> Vec<bool> {
    let mut land = vec![false; board.slots()];
    for h in board.hexes() {
        land[board.index(h).unwrap()] = on_land(proj.to_geo(planar(h)));
    }
    land
}

/// Hexes of land a procedural board for `players` would have, to size Europe the same.
fn target_land(s: &MatchSettings) -> usize {
    let r = s.rules.radius_for_players(s.players) as usize;
    (3 * r * r + 3 * r + 1) * 4 / 5
}

/// The smallest board on which Europe has as much land as a procedural map.
pub fn radius(s: &MatchSettings) -> i32 {
    let want = target_land(s) * 10 / 9;
    let base = s.rules.radius_for_players(s.players);
    (base..base * 3)
        .find(|&r| {
            let board = Board::new(r);
            land_hexes(&board, &Projection::fit(r)).iter().filter(|&&l| l).count() >= want
        })
        .unwrap_or(base * 3)
}

/// Where each city lands on a board of `radius`, if it is on the board.
pub fn places(radius: i32) -> Vec<(&'static str, Hex)> {
    let board = Board::new(radius);
    let proj = Projection::fit(radius);
    PLACES
        .iter()
        .map(|&(name, at)| (name, proj.hex_of(at)))
        .filter(|&(_, h)| board.contains(h))
        .collect()
}

const MOUNTAIN_SHARE_PCT: usize = 9;

fn terrain(board: &Board, proj: &Projection, seed: u64) -> Vec<Option<Terrain>> {
    let hexes = board.hexes();
    let land = land_hexes(board, proj);
    let scale = 2.5 + board.radius as f64 * 0.2;
    let mut coast_dist = vec![u32::MAX; board.slots()];
    let mut q = VecDeque::new();
    for &h in &hexes {
        let i = board.index(h).unwrap();
        if !land[i] {
            coast_dist[i] = 0;
            q.push_back(h);
        }
    }
    while let Some(c) = q.pop_front() {
        let d = coast_dist[board.index(c).unwrap()];
        for nb in c.neighbors() {
            if let Some(j) = board.index(nb) {
                if coast_dist[j] == u32::MAX {
                    coast_dist[j] = d + 1;
                    q.push_back(nb);
                }
            }
        }
    }
    let land_idx: Vec<usize> = hexes
        .iter()
        .map(|&h| board.index(h).unwrap())
        .filter(|&i| land[i])
        .collect();
    let geo = |i: usize| proj.to_geo(planar(board.hex_at(i)));
    let elev: Vec<f64> = (0..board.slots())
        .map(|i| {
            if !land[i] {
                return 0.0;
            }
            let (x, y) = planar(board.hex_at(i));
            let p = geo(i);
            let inland = (coast_dist[i].min(6) as f64) / 6.0;
            1.0 * ridge(p) + 0.6 * upland(p) + 0.35 * fbm(seed ^ 0xE1, x, y, scale, 4) + 0.2 * inland
        })
        .collect();
    let mut order = land_idx.clone();
    order.sort_by(|&a, &b| elev[a].partial_cmp(&elev[b]).unwrap());
    let mut height = vec![0i8; board.slots()];
    let bands = [15usize, 20, 30, 20, 15];
    let mut start = 0;
    for (band, pct) in bands.iter().enumerate() {
        let end = if band == 4 { order.len() } else { start + order.len() * pct / 100 };
        for &i in &order[start..end] {
            height[i] = band as i8 - 2;
        }
        start = end;
    }
    let peak: Vec<f64> = (0..board.slots())
        .map(|i| {
            let (x, y) = planar(board.hex_at(i));
            ridge(geo(i)) + 0.45 * fbm(seed ^ 0xF2, x, y, scale * 0.6, 3)
        })
        .collect();
    let mut by_peak = land_idx.clone();
    by_peak.sort_by(|&a, &b| peak[b].partial_cmp(&peak[a]).unwrap());
    let mut terrain: Vec<Option<Terrain>> = vec![None; board.slots()];
    for &h in &hexes {
        let i = board.index(h).unwrap();
        terrain[i] = Some(if land[i] {
            Terrain { height: height[i], kind: TerrainKind::Open }
        } else {
            Terrain { height: -2, kind: TerrainKind::Water }
        });
    }
    keep_largest_component(board, &mut terrain);
    let mut placed = 0;
    for &i in &by_peak {
        if placed == land_idx.len() * MOUNTAIN_SHARE_PCT / 100 {
            break;
        }
        if ridge(geo(i)) > 0.35 && terrain[i].is_some_and(|t| t.passable()) && !cuts(board, &terrain, i) {
            terrain[i] = Some(Terrain { height: 2, kind: TerrainKind::Mountain });
            placed += 1;
        }
    }
    terrain
}

/// Whether making hex `i` impassable would split the open land around it.
fn cuts(board: &Board, terrain: &[Option<Terrain>], i: usize) -> bool {
    let h = board.hex_at(i);
    let open: Vec<Hex> = h.neighbors().into_iter().filter(|&n| passable(board, terrain, n)).collect();
    let Some(&first) = open.first() else { return false };
    let mut seen = vec![false; board.slots()];
    seen[i] = true;
    seen[board.index(first).unwrap()] = true;
    let mut left = open.len() - 1;
    let mut stack = vec![first];
    while let Some(c) = stack.pop() {
        for nb in c.neighbors() {
            let Some(j) = board.index(nb) else { continue };
            if seen[j] || !passable(board, terrain, nb) {
                continue;
            }
            seen[j] = true;
            if open.contains(&nb) {
                left -= 1;
                if left == 0 {
                    return false;
                }
            }
            stack.push(nb);
        }
    }
    left > 0
}

/// The open hex nearest `from` that makes a fair start, if one is close.
fn start_near(
    board: &Board,
    terrain: &[Option<Terrain>],
    garrison: &[u32],
    from: Hex,
    s: &MatchSettings,
) -> Option<Hex> {
    let mut seen = vec![false; board.slots()];
    let mut q = VecDeque::from([from]);
    seen[board.index(from)?] = true;
    while let Some(c) = q.pop_front() {
        if c.distance(from) > 4 {
            return None;
        }
        if passable(board, terrain, c)
            && open_within(board, terrain, c, 4) >= 14
            && has_two_independent_routes(board, terrain, c)
            && affordable_expansions(board, terrain, garrison, c, &s.rules) >= 2
        {
            return Some(c);
        }
        for nb in c.neighbors() {
            if let Some(j) = board.index(nb) {
                if !seen[j] {
                    seen[j] = true;
                    q.push_back(nb);
                }
            }
        }
    }
    None
}

pub fn generate(s: &MatchSettings) -> Result<GeneratedMap, String> {
    let rules = &s.rules;
    let radius = radius(s);
    let board = Board::new(radius);
    let proj = Projection::fit(radius);
    for attempt in 0..40u64 {
        let tseed = splitmix(s.terrain_seed ^ splitmix(attempt));
        let terrain = terrain(&board, &proj, tseed);
        let open: Vec<Hex> = board.hexes().into_iter().filter(|&h| passable(&board, &terrain, h)).collect();
        let mut garrison = vec![0u32; board.slots()];
        let mut grng = Rng::new(tseed ^ 0x6A09);
        let gscale = 4.0 + radius as f64 * 0.3;
        for &h in &open {
            let (x, y) = planar(h);
            let t = fbm(tseed ^ 0xD4, x, y, gscale, 3);
            let base = rules.neutral_min as f64 + (rules.neutral_max - rules.neutral_min) as f64 * t;
            let jitter = grng.range(0, 4) as i32 - 2;
            garrison[board.index(h).unwrap()] = (base.round() as i32 + jitter)
                .clamp(rules.neutral_min as i32, rules.neutral_max as i32) as u32;
        }
        let min_dist = rules.spawn_min_distance.max(6);
        let mut rng = Rng::new(s.spawn_seed ^ splitmix(attempt.wrapping_add(77)));
        let mut cities: Vec<Hex> = places(radius).into_iter().map(|(_, h)| h).collect();
        rng.shuffle(&mut cities);
        let mut spawns: Vec<Hex> = vec![];
        for &c in &cities {
            if spawns.len() == s.players as usize {
                break;
            }
            if let Some(h) = start_near(&board, &terrain, &garrison, c, s) {
                if spawns.iter().all(|p| p.distance(h) >= min_dist) {
                    spawns.push(h);
                }
            }
        }
        if spawns.len() != s.players as usize {
            continue;
        }
        for &sp in &spawns {
            garrison[board.index(sp).unwrap()] = 0;
        }
        rng.shuffle(&mut spawns);
        return Ok(GeneratedMap {
            radius,
            terrain,
            garrison,
            buildings: vec![None; board.slots()],
            spawns,
        });
    }
    Err("no fair Europe start positions after 40 attempts".into())
}
