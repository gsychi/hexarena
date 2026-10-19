//! Real-world maps, traced by hand in longitude/latitude and laid onto the hex board.
//! Each one is a single connected landmass: islands are left out, and straits that
//! people cross by bridge or tunnel are drawn as one-hex land bridges. Everything that
//! isn't land is impassable sea.

mod africa;
mod china;
mod europe;
mod japan;
mod mediterranean;
mod north_america;
mod usa;

use crate::mapgen::{
    affordable_expansions, has_two_independent_routes, keep_largest_component, open_within,
    passable, planar, GeneratedMap,
};
use crate::rng::{fbm, splitmix, Rng};
use crate::Board;
use hexarena_protocol::{Hex, MapPreset, MatchSettings, Terrain, TerrainKind};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

/// Longitude, latitude in degrees.
pub type Geo = (f64, f64);

/// The hand-drawn data behind one preset.
struct Data {
    name: &'static str,
    blurb: &'static str,
    /// Coastlines; a point is land when it is inside an odd number of rings, so a ring
    /// inside another one is a lake or an inland sea.
    rings: Vec<Vec<Geo>>,
    /// Straits crossed by bridge or tunnel, drawn as land one hex wide.
    bridges: &'static [&'static [Geo]],
    /// Ring points the board must hold; the rest may run off the board edge.
    fits: fn(Geo) -> bool,
    /// Mountain chains as ridge lines.
    ranges: Vec<&'static [Geo]>,
    /// Upland blobs: centre and radius in degrees.
    uplands: Vec<(Geo, f64)>,
    /// Cities used as start positions and map labels.
    places: Vec<(&'static str, Geo)>,
}

pub const PRESETS: [MapPreset; 7] = [
    MapPreset::Europe,
    MapPreset::Mediterranean,
    MapPreset::Japan,
    MapPreset::China,
    MapPreset::UnitedStates,
    MapPreset::NorthAmerica,
    MapPreset::Africa,
];

fn data(p: MapPreset) -> Data {
    match p {
        MapPreset::Europe => europe::data(),
        MapPreset::Mediterranean => mediterranean::data(),
        MapPreset::Japan => japan::data(),
        MapPreset::China => china::data(),
        MapPreset::UnitedStates => usa::data(),
        MapPreset::NorthAmerica => north_america::data(),
        MapPreset::Africa => africa::data(),
    }
}

/// A preset with its projection worked out.
pub struct Atlas {
    data: Data,
    /// Projection centre in radians.
    lam0: f64,
    phi1: f64,
    /// Offset of the board centre in projected units, and the farthest fit point's
    /// hex distance from it at one board unit per projected unit.
    x0: f64,
    y0: f64,
    w: f64,
}

pub fn atlas(p: MapPreset) -> &'static Atlas {
    static ALL: [OnceLock<Atlas>; PRESETS.len()] = [const { OnceLock::new() }; PRESETS.len()];
    let i = PRESETS.iter().position(|&q| q == p).expect("every preset is listed");
    ALL[i].get_or_init(|| Atlas::new(data(p)))
}

pub fn name(p: MapPreset) -> &'static str {
    atlas(p).data.name
}

pub fn blurb(p: MapPreset) -> &'static str {
    atlas(p).data.blurb
}

/// `max(|q|, |r|, |s|)` of a point in planar board units.
fn hex_norm(x: f64, y: f64) -> f64 {
    let r = y / 0.866_025_4;
    let q = x - r / 2.0;
    q.abs().max(r.abs()).max((q + r).abs())
}

/// Lambert azimuthal equal-area, so continents keep their relative sizes.
fn forward(lam0: f64, phi1: f64, (lon, lat): Geo) -> (f64, f64) {
    let (lam, phi) = (lon.to_radians(), lat.to_radians());
    let d = lam - lam0;
    let k = (2.0 / (1.0 + phi1.sin() * phi.sin() + phi1.cos() * phi.cos() * d.cos())).sqrt();
    (k * phi.cos() * d.sin(), k * (phi1.cos() * phi.sin() - phi1.sin() * phi.cos() * d.cos()))
}

fn inverse(lam0: f64, phi1: f64, (x, y): (f64, f64)) -> Geo {
    let rho = (x * x + y * y).sqrt();
    if rho < 1e-12 {
        return (lam0.to_degrees(), phi1.to_degrees());
    }
    let c = 2.0 * (rho / 2.0).min(1.0).asin();
    let phi = (c.cos() * phi1.sin() + y * c.sin() * phi1.cos() / rho).clamp(-1.0, 1.0).asin();
    let lam = lam0 + (x * c.sin()).atan2(rho * phi1.cos() * c.cos() - y * phi1.sin() * c.sin());
    (lam.to_degrees(), phi.to_degrees())
}

impl Atlas {
    fn new(data: Data) -> Atlas {
        let fit: Vec<Geo> = data.rings.iter().flatten().copied().filter(|&p| (data.fits)(p)).collect();
        let (lon_lo, lon_hi, lat_lo, lat_hi) = fit.iter().fold(
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
            |(a, b, c, d), &(lon, lat)| (a.min(lon), b.max(lon), c.min(lat), d.max(lat)),
        );
        let (lam0, phi1) = (((lon_lo + lon_hi) / 2.0).to_radians(), ((lat_lo + lat_hi) / 2.0).to_radians());
        let pts: Vec<(f64, f64)> = fit.iter().map(|&p| forward(lam0, phi1, p)).collect();
        let worst = |x0: f64, y0: f64| pts.iter().map(|&(x, y)| hex_norm(x - x0, y0 - y)).fold(0.0, f64::max);
        let (xs, ys) = (pts.iter().map(|p| p.0), pts.iter().map(|p| p.1));
        let (x_lo, x_hi) = (xs.clone().fold(f64::MAX, f64::min), xs.fold(f64::MIN, f64::max));
        let (y_lo, y_hi) = (ys.clone().fold(f64::MAX, f64::min), ys.fold(f64::MIN, f64::max));
        let (mut cx, mut cy) = ((x_lo + x_hi) / 2.0, (y_lo + y_hi) / 2.0);
        let (mut sx, mut sy) = ((x_hi - x_lo) / 2.0, (y_hi - y_lo) / 2.0);
        let mut best = (worst(cx, cy), cx, cy);
        for _ in 0..3 {
            for i in -20..=20 {
                for j in -20..=20 {
                    let (x0, y0) = (cx + sx * i as f64 / 20.0, cy + sy * j as f64 / 20.0);
                    let w = worst(x0, y0);
                    if w < best.0 {
                        best = (w, x0, y0);
                    }
                }
            }
            (cx, cy, sx, sy) = (best.1, best.2, sx / 10.0, sy / 10.0);
        }
        Atlas { data, lam0, phi1, x0: best.1, y0: best.2, w: best.0 }
    }

    fn projection(&self, radius: i32) -> Projection<'_> {
        Projection { atlas: self, k: (radius as f64 - 1.2) / self.w }
    }

    fn on_land(&self, (lon, lat): Geo) -> bool {
        let mut inside = false;
        for ring in &self.data.rings {
            let mut j = ring.len() - 1;
            for i in 0..ring.len() {
                let ((xi, yi), (xj, yj)) = (ring[i], ring[j]);
                if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
        }
        inside
    }
}

/// Degrees to board units and back, for one board size.
struct Projection<'a> {
    atlas: &'a Atlas,
    k: f64,
}

impl Projection<'_> {
    fn to_board(&self, p: Geo) -> (f64, f64) {
        let a = self.atlas;
        let (x, y) = forward(a.lam0, a.phi1, p);
        ((x - a.x0) * self.k, (a.y0 - y) * self.k)
    }

    fn to_geo(&self, (x, y): (f64, f64)) -> Geo {
        let a = self.atlas;
        inverse(a.lam0, a.phi1, (x / self.k + a.x0, a.y0 - y / self.k))
    }

    /// Board units per degree of latitude.
    fn per_degree(&self) -> f64 {
        self.k * 1f64.to_radians()
    }

    fn hex_at(&self, (x, y): (f64, f64)) -> Hex {
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

    fn hex_of(&self, p: Geo) -> Hex {
        self.hex_at(self.to_board(p))
    }

    /// `line` in board units.
    fn polyline(&self, line: &[Geo]) -> Vec<(f64, f64)> {
        line.iter().map(|&p| self.to_board(p)).collect()
    }
}

fn land_hexes(board: &Board, proj: &Projection) -> Vec<bool> {
    let mut land = vec![false; board.slots()];
    for h in board.hexes() {
        land[board.index(h).unwrap()] = proj.atlas.on_land(proj.to_geo(planar(h)));
    }
    for bridge in proj.atlas.data.bridges {
        for w in proj.polyline(bridge).windows(2) {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            let steps = ((bx - ax).hypot(by - ay) / 0.2).ceil().max(1.0) as usize;
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                if let Some(i) = board.index(proj.hex_at((ax + (bx - ax) * t, ay + (by - ay) * t))) {
                    land[i] = true;
                }
            }
        }
    }
    land
}

/// Every real map gives at least this much land, so small lobbies still get a
/// recognisable coastline.
const MIN_LAND: usize = 480;

/// Hexes of land a procedural board for `players` would have, to size real maps the same.
fn target_land(s: &MatchSettings) -> usize {
    let r = s.rules.radius_for_players(s.players) as usize;
    ((3 * r * r + 3 * r + 1) * 8 / 9).max(MIN_LAND)
}

/// The smallest board on which the map has as much land as a procedural map.
pub fn radius(preset: MapPreset, s: &MatchSettings) -> i32 {
    static CACHE: OnceLock<Mutex<HashMap<(MapPreset, usize), i32>>> = OnceLock::new();
    let want = target_land(s);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(&r) = cache.lock().unwrap().get(&(preset, want)) {
        return r;
    }
    let a = atlas(preset);
    let land = |r: i32| land_hexes(&Board::new(r), &a.projection(r)).iter().filter(|&&l| l).count();
    let base = s.rules.radius_for_players(s.players);
    let first = land(base).max(1);
    let mut r = if first >= want {
        base
    } else {
        (1.2 + (base as f64 - 1.2) * (want as f64 / first as f64).sqrt()).round() as i32
    };
    while r > base && land(r - 1) >= want {
        r -= 1;
    }
    while land(r) < want && r < 200 {
        r += 1;
    }
    cache.lock().unwrap().insert((preset, want), r);
    r
}

/// Where each city lands on a board of `radius`, if it is on the board.
pub fn places(preset: MapPreset, radius: i32) -> Vec<(&'static str, Hex)> {
    let a = atlas(preset);
    let board = Board::new(radius);
    let proj = a.projection(radius);
    a.data
        .places
        .iter()
        .map(|&(name, at)| (name, proj.hex_of(at)))
        .filter(|&(_, h)| board.contains(h))
        .collect()
}

/// Distance in board units from a point to a polyline.
fn to_line((px, py): (f64, f64), line: &[(f64, f64)]) -> f64 {
    let mut best = f64::MAX;
    for w in line.windows(2) {
        let ((ax, ay), (bx, by)) = (w[0], w[1]);
        let (dx, dy) = (bx - ax, by - ay);
        let len = dx * dx + dy * dy;
        let t = if len == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len).clamp(0.0, 1.0) };
        best = best.min((px - ax - t * dx).hypot(py - ay - t * dy));
    }
    best
}

/// Ridge lines and upland blobs in board units, with their widths.
struct Relief {
    ranges: Vec<Vec<(f64, f64)>>,
    ridge_width: f64,
    uplands: Vec<((f64, f64), f64)>,
}

impl Relief {
    fn new(proj: &Projection) -> Relief {
        let deg = proj.per_degree();
        Relief {
            ranges: proj.atlas.data.ranges.iter().map(|l| proj.polyline(l)).collect(),
            ridge_width: (0.75 * deg).max(1.0),
            uplands: proj.atlas.data.uplands.iter().map(|&(c, r)| (proj.to_board(c), (r * deg).max(1.5))).collect(),
        }
    }

    fn ridge(&self, p: (f64, f64)) -> f64 {
        self.ranges.iter().map(|l| (-(to_line(p, l) / self.ridge_width).powi(2)).exp()).fold(0.0, f64::max)
    }

    fn upland(&self, (x, y): (f64, f64)) -> f64 {
        self.uplands
            .iter()
            .map(|&((cx, cy), r)| (-((x - cx).hypot(y - cy) / r).powi(2)).exp())
            .fold(0.0, f64::max)
    }
}

const MOUNTAIN_SHARE_PCT: usize = 9;

fn terrain(board: &Board, proj: &Projection, seed: u64) -> Vec<Option<Terrain>> {
    let hexes = board.hexes();
    let land = land_hexes(board, proj);
    let relief = Relief::new(proj);
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
    let land_idx: Vec<usize> = hexes.iter().map(|&h| board.index(h).unwrap()).filter(|&i| land[i]).collect();
    let mut ridge = vec![0.0; board.slots()];
    let mut elev = vec![0.0; board.slots()];
    for &i in &land_idx {
        let p = planar(board.hex_at(i));
        ridge[i] = relief.ridge(p);
        let inland = (coast_dist[i].min(6) as f64) / 6.0;
        elev[i] = ridge[i] + 0.6 * relief.upland(p) + 0.35 * fbm(seed ^ 0xE1, p.0, p.1, scale, 4) + 0.2 * inland;
    }
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
    let peak = |i: usize| {
        let (x, y) = planar(board.hex_at(i));
        ridge[i] + 0.45 * fbm(seed ^ 0xF2, x, y, scale * 0.6, 3)
    };
    let mut by_peak: Vec<(f64, usize)> = land_idx.iter().map(|&i| (peak(i), i)).collect();
    by_peak.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
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
    for &(_, i) in &by_peak {
        if placed == land_idx.len() * MOUNTAIN_SHARE_PCT / 100 {
            break;
        }
        if ridge[i] > 0.35 && terrain[i].is_some_and(|t| t.passable()) && !cuts(board, &terrain, i) {
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
    if left == 0 {
        return false;
    }
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
fn start_near(board: &Board, terrain: &[Option<Terrain>], garrison: &[u32], from: Hex, s: &MatchSettings) -> Option<Hex> {
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

pub fn generate(preset: MapPreset, s: &MatchSettings) -> Result<GeneratedMap, String> {
    let rules = &s.rules;
    let radius = radius(preset, s);
    let board = Board::new(radius);
    let proj = atlas(preset).projection(radius);
    let cities: Vec<Hex> = places(preset, radius).into_iter().map(|(_, h)| h).collect();
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
            garrison[board.index(h).unwrap()] =
                (base.round() as i32 + jitter).clamp(rules.neutral_min as i32, rules.neutral_max as i32) as u32;
        }
        let min_dist = rules.spawn_min_distance.max(6);
        let mut rng = Rng::new(s.spawn_seed ^ splitmix(attempt.wrapping_add(77)));
        let mut order = cities.clone();
        rng.shuffle(&mut order);
        let mut spawns: Vec<Hex> = vec![];
        for &c in &order {
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
        return Ok(GeneratedMap { radius, terrain, garrison, buildings: vec![None; board.slots()], spawns });
    }
    Err(format!("no fair {} start positions after 40 attempts", atlas(preset).data.name))
}

