//! The lands around the Mediterranean, with the sea and the Black Sea in the middle.
//! The Strait of Gibraltar, the Dardanelles, the Bosporus and Suez are land, so Europe,
//! Asia and Africa make one ring; Sicily, Sardinia, Corsica, Crete and Cyprus are left out.

use super::{europe, Data, Geo};

pub(super) fn data() -> Data {
    let mut ranges = europe::RANGES.to_vec();
    ranges.extend_from_slice(RANGES);
    let mut uplands = europe::UPLANDS.to_vec();
    uplands.extend_from_slice(UPLANDS);
    Data {
        name: "Mediterranean",
        blurb: "Spain to the Levant and Morocco to Egypt around the sea. Gibraltar, the Bosporus and Suez are the crossings.",
        rings: vec![outer(), sea(), black_sea()],
        bridges: &[&[(-5.78, 35.78), (-5.6, 36.05)]],
        fits,
        ranges,
        uplands,
        places: PLACES.to_vec(),
    }
}

fn fits((lon, lat): Geo) -> bool {
    (-12.0..42.5).contains(&lon) && (29.0..46.8).contains(&lat)
}

/// The points of Europe's coast from `from` to `to`, inclusive.
fn europe_between(from: Geo, to: Geo) -> Vec<Geo> {
    let at = |p: Geo| europe::COAST.iter().position(|&q| q == p).expect("a point on the Europe coast");
    europe::COAST[at(from)..=at(to)].to_vec()
}

/// Land around the Atlantic side, closed off the board to the north, east and south.
fn outer() -> Vec<Geo> {
    let mut ring = vec![
        (-14.0, 26.0), (-12.9, 27.9), (-11.1, 28.5), (-10.17, 29.38), (-9.65, 30.4), (-9.85, 31.0),
        (-9.75, 31.5), (-9.25, 32.3), (-8.5, 33.25), (-7.6, 33.6), (-6.85, 34.0), (-6.6, 34.3),
        (-6.15, 35.2), (-5.92, 35.79), (-5.7, 36.05),
    ];
    let mut iberia = europe_between((-4.7, 48.45), (-8.8, 37.4));
    let mut south = europe_between((-8.9, 37.0), (-6.2, 36.3));
    south.reverse();
    ring.extend(south);
    iberia.reverse();
    ring.extend(iberia);
    ring.extend([(-4.7, 52.0), (45.0, 52.0), (45.0, 26.0)]);
    ring
}

fn sea() -> Vec<Geo> {
    let mut ring = europe_between((-5.6, 36.0), (26.3, 40.1));
    ring.extend_from_slice(SOUTH_AND_EAST_SHORE);
    ring
}

/// From the Dardanelles down the Turkish coast, round the Levant and west along Africa.
pub(super) const SOUTH_AND_EAST_SHORE: &[Geo] = &[
    (26.15, 39.95), (26.07, 39.48), (26.8, 38.9), (26.9, 38.45), (26.3, 38.3), (26.6, 38.0),
    (27.2, 37.9), (27.3, 37.4), (27.4, 37.03), (28.25, 36.85), (29.1, 36.6), (29.6, 36.2),
    (30.5, 36.3), (30.65, 36.85), (31.5, 36.6), (32.0, 36.5), (32.8, 36.05), (33.9, 36.25),
    (34.6, 36.8), (35.5, 36.6), (36.15, 36.85), (35.9, 36.25), (35.78, 35.52), (35.9, 34.9),
    (35.85, 34.45), (35.5, 33.9), (35.2, 33.25), (34.95, 32.8), (34.75, 32.1), (34.45, 31.5),
    (33.8, 31.15), (32.3, 31.25), (31.9, 31.5), (31.0, 31.6), (30.3, 31.45), (29.9, 31.2),
    (28.95, 30.83), (27.25, 31.35), (25.15, 31.55), (23.95, 32.08), (22.65, 32.77), (21.7, 32.9),
    (20.6, 32.6), (20.07, 32.1), (19.9, 31.5), (20.1, 31.0), (19.6, 30.4), (19.1, 30.25),
    (18.0, 30.7), (16.6, 31.2), (15.6, 31.5), (15.2, 32.35), (14.4, 32.65), (13.2, 32.9),
    (12.0, 32.9), (11.1, 33.2), (10.9, 33.75), (10.1, 33.9), (10.45, 34.4), (10.75, 34.75),
    (11.1, 35.25), (10.6, 35.8), (10.5, 36.4), (11.05, 37.05), (10.3, 36.9), (9.85, 37.3),
    (8.9, 36.95), (7.75, 36.9), (6.6, 37.05), (5.75, 36.85), (5.1, 36.75), (3.06, 36.77),
    (2.2, 36.6), (1.3, 36.5), (0.1, 35.9), (-0.65, 35.75), (-1.4, 35.3), (-2.2, 35.1),
    (-2.9, 35.25), (-3.9, 35.2), (-4.8, 35.25), (-5.35, 35.9),
];

fn black_sea() -> Vec<Geo> {
    let mut ring = europe_between((29.05, 41.1), (31.5, 46.6));
    ring.extend_from_slice(&[
        (32.0, 46.55), (32.6, 46.05), (33.7, 46.15), (32.5, 45.4), (33.5, 44.6), (34.3, 44.45),
        (35.4, 45.0), (36.6, 45.4), (36.8, 45.2), (37.8, 44.7), (39.7, 43.6), (41.0, 43.0),
        (41.6, 41.6), (40.5, 41.0), (39.7, 41.0), (37.9, 41.0), (36.3, 41.3), (35.15, 42.0),
        (33.75, 41.98), (31.8, 41.45), (30.4, 41.2), (29.15, 41.2),
    ]);
    ring
}

pub(super) const RANGES: &[&[Geo]] = &[
    // High Atlas
    &[(-9.0, 30.8), (-7.5, 31.2), (-5.5, 32.2), (-4.0, 32.6)],
    // Middle Atlas
    &[(-5.0, 33.5), (-4.0, 34.2)],
    // Rif
    &[(-5.3, 35.1), (-4.0, 35.0)],
    // Tell Atlas and Aures
    &[(0.0, 35.4), (2.0, 36.0), (4.0, 36.3), (6.0, 36.3), (6.5, 35.3)],
    // Taurus
    &[(29.0, 37.0), (31.0, 37.3), (33.0, 37.0), (35.0, 37.5), (37.0, 38.0)],
    // Pontic Mountains
    &[(33.0, 41.0), (36.0, 40.6), (40.0, 40.5)],
    // Lebanon
    &[(35.8, 33.9), (36.3, 34.4)],
    // Caucasus
    &[(40.0, 43.4), (42.0, 42.8), (44.5, 42.6)],
];

const UPLANDS: &[(Geo, f64)] = &[
    ((33.0, 39.0), 2.5),  // Anatolian plateau
    ((-2.0, 33.5), 1.5),  // Moroccan and Algerian high plateaus
    ((21.5, 32.5), 0.6),  // Jebel Akhdar
    ((34.0, 29.0), 0.7),  // Sinai
];

const PLACES: &[(&str, Geo)] = &[
    ("Rome", (12.5, 41.9)), ("Madrid", (-3.7, 40.42)), ("Barcelona", (2.17, 41.39)),
    ("Marseille", (5.37, 43.3)), ("Athens", (23.73, 37.98)), ("Istanbul", (28.97, 41.05)),
    ("Ankara", (32.85, 39.93)), ("Cairo", (31.24, 30.04)), ("Alexandria", (29.92, 31.2)),
    ("Tunis", (10.18, 36.8)), ("Algiers", (3.06, 36.75)), ("Tripoli", (13.19, 32.89)),
    ("Benghazi", (20.07, 32.12)), ("Casablanca", (-7.59, 33.57)), ("Fez", (-5.0, 34.03)),
    ("Marrakesh", (-8.0, 31.63)), ("Oran", (-0.64, 35.7)), ("Beirut", (35.5, 33.89)),
    ("Damascus", (36.29, 33.51)), ("Jerusalem", (35.21, 31.77)), ("Izmir", (27.14, 38.42)),
    ("Antalya", (30.71, 36.9)), ("Naples", (14.27, 40.85)), ("Milan", (9.19, 45.46)),
    ("Lisbon", (-9.14, 38.72)), ("Seville", (-5.98, 37.39)), ("Valencia", (-0.38, 39.47)),
    ("Belgrade", (20.46, 44.8)), ("Sofia", (23.32, 42.7)), ("Thessaloniki", (22.94, 40.64)),
    ("Aleppo", (37.16, 36.2)), ("Adana", (35.32, 37.0)), ("Venice", (12.33, 45.44)),
    ("Bucharest", (26.1, 44.43)), ("Zagreb", (15.98, 45.81)),
];
