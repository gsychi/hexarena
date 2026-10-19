use super::*;
use mapgen::{affordable_expansions, generate, has_two_independent_routes};

fn settings(players: u16, seed: u64) -> MatchSettings {
    MatchSettings {
        players,
        terrain_seed: seed,
        spawn_seed: seed ^ 0xFEED,
        match_seed: seed + 3,
        rules: RulesConfig::default(),
    }
}

fn state(players: u16, seed: u64) -> State {
    State::new(settings(players, seed), vec![]).unwrap()
}

fn noop_all(n: u16) -> Vec<(PlayerId, PlayerCommand)> {
    (0..n).map(|p| (p, PlayerCommand::default())).collect()
}

fn act(p: PlayerId, a: Action) -> Vec<(PlayerId, PlayerCommand)> {
    vec![(p, PlayerCommand::action(a))]
}

fn first_owned(st: &State, p: PlayerId) -> Hex {
    st.board
        .hexes()
        .into_iter()
        .find(|&h| st.cell(h).unwrap().owner == Some(p))
        .unwrap()
}

fn open_neighbor(st: &State, h: Hex) -> Hex {
    h.neighbors()
        .into_iter()
        .find(|&n| passable(&st.board, &st.terrain, n))
        .unwrap()
}

fn set(st: &mut State, h: Hex, owner: Option<PlayerId>, troops: u32) {
    let c = st.cell_mut(h);
    c.owner = owner;
    c.troops = troops;
}

fn set_height(st: &mut State, h: Hex, height: i8) {
    let i = st.board.index(h).unwrap();
    st.terrain[i] = Some(Terrain {
        height,
        kind: TerrainKind::Open,
    });
}

fn put_building(st: &mut State, h: Hex, b: Building) {
    let c = st.cell_mut(h);
    c.building = Some(b);
    c.built_tick = 0;
}

/// An interior hex whose whole radius-2 disc is open.
fn interior_open(st: &State) -> Hex {
    st.board
        .hexes()
        .into_iter()
        .find(|&h| {
            st.board.hexes_within(h, 2).len() == 19
                && st
                    .board
                    .hexes_within(h, 2)
                    .iter()
                    .all(|&n| passable(&st.board, &st.terrain, n))
        })
        .expect("open disc")
}

/// An on-board hex far from `c` (used to keep a second player alive).
fn faraway(st: &State, c: Hex) -> Hex {
    st.board
        .hexes()
        .into_iter()
        .max_by_key(|h| (h.distance(c), *h))
        .unwrap()
}

fn clear_owners(st: &mut State) {
    for c in st.cells.iter_mut() {
        c.owner = None;
        c.troops = 0;
    }
}

// ---------- board and generation ----------

#[test]
fn hex_count_formula() {
    for r in 1..25 {
        assert_eq!(Board::new(r).hexes().len() as i32, 1 + 3 * r * (r + 1));
    }
}

#[test]
fn radius_scaling() {
    let r = RulesConfig::default();
    let radii: Vec<i32> = (2..=8).map(|n| r.radius_for_players(n)).collect();
    assert_eq!(radii, vec![11, 12, 13, 14, 14, 15, 16]);
    let v05 = RulesConfig { map_radius_factor: 7.5, map_radius_exponent: 0.5, ..r };
    assert_eq!((v05.radius_for_players(4), v05.radius_for_players(8)), (15, 21));
}

#[test]
fn generation_is_deterministic_and_seed_sensitive() {
    let a = generate(&settings(4, 1)).unwrap();
    let b = generate(&settings(4, 1)).unwrap();
    let c = generate(&settings(4, 2)).unwrap();
    assert_eq!(a.spawns, b.spawns);
    assert_eq!(a.terrain, b.terrain);
    assert_ne!(a.terrain, c.terrain);
}

#[test]
fn spawn_seed_is_independent_of_terrain_seed() {
    let mut s = settings(4, 5);
    let a = generate(&s).unwrap();
    s.spawn_seed ^= 0xABCDEF;
    let b = generate(&s).unwrap();
    assert_eq!(a.terrain, b.terrain);
    assert_ne!(a.spawns, b.spawns);
}

#[test]
fn map_safety_invariants() {
    for players in [2u16, 3, 4, 6, 8] {
        for seed in 0..5u64 {
            let s = settings(players, seed);
            let m = generate(&s).unwrap_or_else(|e| panic!("{players}p seed {seed}: {e}"));
            let board = Board::new(m.radius);
            let hexes = board.hexes();
            let open: Vec<Hex> = hexes
                .iter()
                .copied()
                .filter(|&h| passable(&board, &m.terrain, h))
                .collect();
            let imp = (hexes.len() - open.len()) * 100 / hexes.len();
            assert!((15..=25).contains(&imp), "impassable {imp}%");
            let mut seen = std::collections::HashSet::from([open[0]]);
            let mut stack = vec![open[0]];
            while let Some(c) = stack.pop() {
                for n in c.neighbors() {
                    if passable(&board, &m.terrain, n) && seen.insert(n) {
                        stack.push(n);
                    }
                }
            }
            assert_eq!(seen.len(), open.len(), "disconnected traversable cells");
            assert_eq!(m.spawns.len(), players as usize);
            for (i, &sp) in m.spawns.iter().enumerate() {
                assert!(passable(&board, &m.terrain, sp));
                assert!(has_two_independent_routes(&board, &m.terrain, sp));
                for &o in &m.spawns[i + 1..] {
                    assert!(sp.distance(o) >= 6);
                }
            }
            for t in m.terrain.iter().flatten() {
                assert!((-2..=2).contains(&t.height));
            }
        }
    }
}

#[test]
fn each_spawn_has_legal_affordable_expansion() {
    let rules = RulesConfig::default();
    for players in [2u16, 4, 8] {
        for seed in 0..4u64 {
            let m = generate(&settings(players, seed)).unwrap();
            let board = Board::new(m.radius);
            for &sp in &m.spawns {
                assert!(affordable_expansions(&board, &m.terrain, &m.garrison, sp, &rules) >= 2);
            }
        }
    }
}

#[test]
fn eight_player_map_scale() {
    let m = generate(&settings(8, 42)).unwrap();
    let board = Board::new(m.radius);
    let total = board.hexes().len();
    let open = board
        .hexes()
        .iter()
        .filter(|&&h| passable(&board, &m.terrain, h))
        .count();
    assert_eq!(total, 817);
    assert!((600..=700).contains(&open), "{open}");
}

#[test]
fn compact_boards_still_fit_every_lobby_size() {
    for players in 2..=8u16 {
        for seed in 0..40u64 {
            generate(&settings(players, seed)).unwrap_or_else(|e| panic!("{players}p seed {seed}: {e}"));
        }
    }
}

#[test]
fn neutral_garrisons_are_in_range_and_spawns_start_equal() {
    for seed in 0..4u64 {
        let st = state(6, seed);
        let r = st.rules();
        for h in st.board.hexes() {
            let c = st.cell(h).unwrap();
            if !passable(&st.board, &st.terrain, h) {
                continue;
            }
            match c.owner {
                None => assert!(
                    (r.neutral_min..=r.neutral_max).contains(&c.troops),
                    "{:?} {}",
                    h,
                    c.troops
                ),
                Some(_) => assert_eq!(c.troops, r.starting_troops),
            }
        }
    }
}

// ---------- production ----------

#[test]
fn neutral_land_produces_nothing_and_owned_land_exactly_one() {
    let mut st = state(2, 3);
    let neutral = st
        .board
        .hexes()
        .into_iter()
        .find(|&h| passable(&st.board, &st.terrain, h) && st.cell(h).unwrap().owner.is_none())
        .unwrap();
    let before_n = st.cell(neutral).unwrap().troops;
    let sp = first_owned(&st, 0);
    let before = st.cell(sp).unwrap().troops;
    for h in [-2i8, 2] {
        set_height(&mut st, sp, h);
        let t = st.cell(sp).unwrap().troops;
        st.step(&noop_all(2));
        assert_eq!(st.cell(sp).unwrap().troops, t + 1, "height {h}");
    }
    assert_eq!(st.cell(neutral).unwrap().troops, before_n);
    assert!(st.cell(sp).unwrap().troops > before);
}

// ---------- actions ----------

#[test]
fn impassable_land_cannot_be_entered_or_owned() {
    let mut st = state(2, 5);
    let sp = first_owned(&st, 0);
    let target = open_neighbor(&st, sp);
    let i = st.board.index(target).unwrap();
    st.terrain[i] = Some(Terrain {
        height: 0,
        kind: TerrainKind::Mountain,
    });
    st.cell_mut(sp).troops = 30;
    st.step(&act(
        0,
        Action::Move {
            from: sp,
            to: target,
            troops: 10,
        },
    ));
    assert!(st
        .last_events
        .iter()
        .any(|e| matches!(e, Event::Rejected { player: 0, .. })));
    assert_eq!(st.cell(sp).unwrap().troops, 31);
    for h in st.board.hexes() {
        if !passable(&st.board, &st.terrain, h) {
            assert!(st.cell(h).unwrap().owner.is_none());
        }
    }
}

#[test]
fn initiative_rotates_deterministically() {
    let mut st = state(4, 8);
    let first = st.initiative_order();
    assert_eq!(first.len(), 4);
    let again = st.clone().initiative_order();
    assert_eq!(first, again);
    let mut prev = first;
    for _ in 0..5 {
        st.step(&noop_all(4));
        let now = st.initiative_order();
        let mut expect = prev.clone();
        expect.rotate_left(1);
        assert_eq!(now, expect);
        prev = now;
    }
}

#[test]
fn input_order_does_not_change_outcome() {
    let a = state(3, 2);
    let (mut x, mut y) = (a.clone(), a.clone());
    let cmds: Vec<_> = (0..3u16)
        .map(|p| {
            let from = first_owned(&a, p);
            (
                p,
                PlayerCommand::action(Action::Move {
                    from,
                    to: open_neighbor(&a, from),
                    troops: 20,
                }),
            )
        })
        .collect();
    let mut rev = cmds.clone();
    rev.reverse();
    x.step(&cmds);
    y.step(&rev);
    assert_eq!(x.state_hash(), y.state_hash());
}

#[test]
fn missing_command_is_noop() {
    let a = state(3, 4);
    let (mut x, mut y) = (a.clone(), a.clone());
    x.step(&[]);
    y.step(&noop_all(3));
    assert_eq!(x.state_hash(), y.state_hash());
}

#[test]
fn friendly_move_transfers_and_leaves_one() {
    let mut st = state(2, 6);
    let a = first_owned(&st, 0);
    let b = open_neighbor(&st, a);
    set(&mut st, a, Some(0), 20);
    set(&mut st, b, Some(0), 5);
    st.step(&act(
        0,
        Action::Move {
            from: a,
            to: b,
            troops: 19,
        },
    ));
    assert_eq!(st.cell(a).unwrap().troops, 2);
    assert_eq!(st.cell(b).unwrap().troops, 25);
    assert!(st.cell(b).unwrap().owner == Some(0));
    st.step(&act(
        0,
        Action::Move {
            from: a,
            to: b,
            troops: st.cell(a).unwrap().troops,
        },
    ));
    assert!(st
        .last_events
        .iter()
        .any(|e| matches!(e, Event::Rejected { .. })));
}

// ---------- combat ----------

#[test]
fn height_multipliers_match_reference_examples() {
    let r = RulesConfig::default();
    assert_eq!(r.height_multiplier_milli(0, 0), 1000);
    assert_eq!(r.height_multiplier_milli(0, -1), 667);
    assert_eq!(r.height_multiplier_milli(0, -2), 445);
    assert_eq!(r.height_multiplier_milli(0, 1), 1500);
    assert_eq!(r.height_multiplier_milli(0, 2), 2250);
}

#[test]
fn combat_is_deterministic_and_height_changes_outcomes() {
    let run = |def_h: i8, seed: u64| {
        let mut st = state(2, 9);
        st.rng = rng::Rng::new(seed);
        let (left, def) = st.fight(25, 15, st.rules().defense_milli(0, def_h, None));
        (left, def)
    };
    assert_eq!(run(0, 1), run(0, 1));
    let wins = |h: i8| (0..300u64).filter(|&s| run(h, s).1 == 0).count();
    assert!(wins(-2) > wins(0), "valleys easier");
    assert!(wins(0) > wins(2), "hills harder");
}

#[test]
fn wall_links_defense_and_cap() {
    let mut st = state(2, 7);
    clear_owners(&mut st);
    let c = interior_open(&st);
    let (a, b, d) = (c, c.add(HEX_DIRECTIONS[0]), c.add(HEX_DIRECTIONS[3]));
    for h in [a, b, d] {
        set(&mut st, h, Some(0), 1);
        put_building(&mut st, h, Building::Wall);
    }
    set(&mut st, c.add(HEX_DIRECTIONS[1]), Some(1), 5);
    for h in c.neighbors().into_iter().chain([c]) {
        set_height(&mut st, h, 0);
    }
    assert_eq!(st.defense_milli(a, 0), 2000);
    let link = |x, y, en| PlayerCommand {
        management: vec![ManagementCommand::SetWallLink {
            wall_a: x,
            wall_b: y,
            enabled: en,
        }],
        action: Action::NoOp,
    };
    st.step(&[(0, link(a, b, true))]);
    assert_eq!(st.wall_links(a), 1);
    assert_eq!(st.defense_milli(a, 0), 2500);
    st.tick += 20;
    st.step(&[(0, link(a, d, true))]);
    assert_eq!(st.defense_milli(a, 0), 3000);
    assert_eq!(st.defense_milli(b, 0), 2500);
    // more links are rejected
    let e = c.add(HEX_DIRECTIONS[5]);
    set(&mut st, e, Some(0), 1);
    put_building(&mut st, e, Building::Wall);
    st.tick += 20;
    st.step(&[(0, link(a, e, true))]);
    assert!(st
        .last_events
        .iter()
        .any(|ev| matches!(ev, Event::Rejected { .. })));
    assert_eq!(st.wall_links(a), 2);
    // cap: hill (+2) attacked from valley (-2) with 3x wall = 5.06 * 3 -> capped at 5.0
    set_height(&mut st, a, 2);
    assert_eq!(st.defense_milli(a, -2), 5000);
    // cooldown blocks immediate re-link
    st.step(&[(0, link(a, d, false))]);
    assert!(
        st.last_events
            .iter()
            .any(|ev| matches!(ev, Event::Rejected { .. }))
            || st.wall_links(a) == 1
    );
}

#[test]
fn capture_transfers_buildings_and_breaks_links() {
    let mut st = state(2, 11);
    clear_owners(&mut st);
    let c = interior_open(&st);
    let farm = c;
    let village = c.add(HEX_DIRECTIONS[0]);
    let attacker = c.add(HEX_DIRECTIONS[0]).add(HEX_DIRECTIONS[0]);
    for h in [c, c.add(HEX_DIRECTIONS[0]), attacker] {
        set_height(&mut st, h, 0);
    }
    set(&mut st, farm, Some(1), 5);
    set(&mut st, village, Some(1), 1);
    set(&mut st, attacker, Some(0), 200);
    set(&mut st, c.add(HEX_DIRECTIONS[3]), Some(1), 1);
    put_building(&mut st, farm, Building::Farm);
    put_building(&mut st, village, Building::Village);
    st.cell_mut(farm).links[0] = Some(village);
    assert_eq!(st.supporting_farm(village), Some(farm));
    st.step(&act(
        0,
        Action::Move {
            from: attacker,
            to: village,
            troops: 150,
        },
    ));
    let v = st.cell(village).unwrap();
    assert_eq!(v.owner, Some(0));
    assert_eq!(v.building, Some(Building::Village));
    assert!(st.cell(farm).unwrap().links.iter().all(|l| l.is_none()));
    assert_eq!(st.supporting_farm(village), None);
    assert!(st
        .last_events
        .iter()
        .any(|e| matches!(e, Event::LinksBroken { .. })));
}

#[test]
fn wall_links_break_on_capture() {
    let mut st = state(2, 12);
    clear_owners(&mut st);
    let c = interior_open(&st);
    let (a, b) = (c, c.add(HEX_DIRECTIONS[0]));
    let attacker = b.add(HEX_DIRECTIONS[0]);
    set(&mut st, a, Some(1), 1);
    set(&mut st, b, Some(1), 1);
    set(&mut st, c.add(HEX_DIRECTIONS[3]), Some(1), 1);
    set(&mut st, attacker, Some(0), 300);
    for h in [a, b] {
        put_building(&mut st, h, Building::Wall);
    }
    st.cell_mut(a).links[0] = Some(b);
    st.cell_mut(b).links[0] = Some(a);
    set_height(&mut st, b, 0);
    set_height(&mut st, attacker, 0);
    st.step(&act(
        0,
        Action::Move {
            from: attacker,
            to: b,
            troops: 299,
        },
    ));
    assert_eq!(st.cell(b).unwrap().owner, Some(0));
    assert_eq!(st.cell(b).unwrap().building, Some(Building::Wall));
    assert_eq!(st.wall_links(a), 0);
    assert_eq!(st.wall_links(b), 0);
}

// ---------- gather ----------

#[test]
fn gather_moves_simultaneously_one_step() {
    let mut st = state(2, 13);
    clear_owners(&mut st);
    let c = interior_open(&st);
    let line: Vec<Hex> = (0..4).map(|i| Hex::new(c.q + i, c.r)).collect();
    for (i, &h) in line.iter().enumerate() {
        set(&mut st, h, Some(0), if i == 0 { 1 } else { 5 });
    }
    let other = Hex::new(c.q, c.r + 2);
    set(&mut st, other, Some(1), 3);
    st.step(&act(0, Action::Gather { target: line[0] }));
    let t: Vec<u32> = line.iter().map(|&h| st.cell(h).unwrap().troops).collect();
    // after gather: [1+4, 1+4, 1+4, 1] then +1 production each
    assert_eq!(t, vec![6, 6, 6, 2]);
}

#[test]
fn gather_does_not_cross_enemy_or_unowned_land() {
    let mut st = state(2, 14);
    clear_owners(&mut st);
    let c = interior_open(&st);
    let (a, b, far) = (c, Hex::new(c.q + 1, c.r), Hex::new(c.q + 3, c.r));
    set(&mut st, a, Some(0), 1);
    set(&mut st, b, Some(0), 4);
    set(&mut st, far, Some(0), 9);
    set(&mut st, Hex::new(c.q + 2, c.r), Some(1), 2);
    st.step(&act(0, Action::Gather { target: a }));
    assert_eq!(st.cell(far).unwrap().troops, 10);
    assert_eq!(st.cell(a).unwrap().troops, 1 + 3 + 1);
}

// ---------- encirclement ----------

#[test]
fn enclosure_requires_all_boundaries_owned_by_capturer() {
    let mut st = state(3, 15);
    clear_owners(&mut st);
    let c = interior_open(&st);
    set(&mut st, c, Some(1), 1);
    for n in c.neighbors() {
        set(&mut st, n, Some(0), 1);
    }
    assert!(st.is_enclosed(&[c], 0));
    let n0 = c.neighbors()[0];
    set(&mut st, n0, None, 5);
    assert!(!st.is_enclosed(&[c], 0), "neutral boundary");
    set(&mut st, n0, Some(2), 5);
    assert!(!st.is_enclosed(&[c], 0), "third player boundary");
    set(&mut st, n0, Some(0), 1);
    set_impassable(&mut st, n0);
    assert!(!st.is_enclosed(&[c], 0), "impassable breaks enclosure");
    st.settings.rules.impassable_breaks_encirclement = false;
    assert!(
        st.is_enclosed(&[c], 0),
        "when configured, impassable is ignored"
    );
    let edge = st
        .board
        .hexes()
        .into_iter()
        .find(|h| h.q.abs().max(h.r.abs()).max((h.q + h.r).abs()) == st.board.radius)
        .unwrap();
    assert!(!st.is_enclosed(&[edge], 0), "map edge");
}

fn set_impassable(st: &mut State, h: Hex) {
    let i = st.board.index(h).unwrap();
    st.terrain[i] = Some(Terrain {
        height: 0,
        kind: TerrainKind::Mountain,
    });
}

#[test]
fn capture_surrenders_enclosed_enemy_component() {
    let mut st = state(3, 16);
    clear_owners(&mut st);
    let c = interior_open(&st);
    set(&mut st, c, Some(1), 2);
    let x = c.neighbors()[0];
    for n in c.neighbors() {
        set(&mut st, n, Some(0), 40);
    }
    set(&mut st, x, None, 1);
    let f = faraway(&st, c);
    set(&mut st, f, Some(1), 3);
    st.step(&act(
        0,
        Action::Move {
            from: c.neighbors()[1],
            to: x,
            troops: 30,
        },
    ));
    assert_eq!(st.cell(x).unwrap().owner, Some(0));
    assert_eq!(st.cell(c).unwrap().owner, Some(0));
    assert!(st.last_events.iter().any(|e| matches!(
        e,
        Event::Surrendered {
            capturer: 0,
            from: 1,
            ..
        }
    )));
    assert_eq!(
        st.cell(c).unwrap().troops,
        2 + 1,
        "troops remain, plus production"
    );
}

/// Owner 1 on `c`, neutral (7 troops) on its neighbour `d`, and player 0 on every hex
/// around the pair except `gap`, which is neutral. Returns (c, d, gap, ring).
fn ringed_pair(st: &mut State) -> (Hex, Hex, Hex, Vec<Hex>) {
    clear_owners(st);
    let c = interior_open(st);
    let d = c.neighbors()[0];
    let mut ring: Vec<Hex> = c
        .neighbors()
        .into_iter()
        .chain(d.neighbors())
        .filter(|h| *h != c && *h != d)
        .collect();
    ring.sort_unstable();
    ring.dedup();
    set(st, c, Some(1), 4);
    set(st, d, None, 7);
    for &h in &ring {
        set(st, h, Some(0), 40);
    }
    let gap = ring[0];
    set(st, gap, None, 1);
    let f = faraway(st, c);
    set(st, f, Some(1), 3);
    (c, d, gap, ring)
}

fn close_gap(st: &mut State, gap: Hex, ring: &[Hex]) {
    let from = *ring
        .iter()
        .find(|h| h.distance(gap) == 1 && **h != gap)
        .unwrap();
    st.step(&act(0, Action::Move { from, to: gap, troops: 30 }));
    assert_eq!(st.cell(gap).unwrap().owner, Some(0));
}

#[test]
fn closing_a_ring_captures_everything_inside_it_like_go() {
    let mut st = state(3, 16);
    let (c, d, gap, ring) = ringed_pair(&mut st);
    assert!(st.settings.rules.region_capture);
    close_gap(&mut st, gap, &ring);
    assert_eq!(st.cell(c).unwrap().owner, Some(0), "enemy hex inside");
    assert_eq!(st.cell(d).unwrap().owner, Some(0), "neutral hex inside");
    assert_eq!(st.cell(d).unwrap().troops, 7 + 1, "troops kept, plus production");
    let ev = &st.last_events;
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Surrendered { capturer: 0, from: 1, hexes } if hexes == &vec![c]
    )));
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Captured { player: 0, at, previous_owner: None, .. } if *at == d
    )));
}

#[test]
fn a_ring_with_an_impassable_hex_inside_still_captures_the_open_hexes() {
    let mut st = state(3, 16);
    let (c, d, gap, ring) = ringed_pair(&mut st);
    set(&mut st, d, None, 0);
    set_impassable(&mut st, d);
    close_gap(&mut st, gap, &ring);
    assert_eq!(st.cell(c).unwrap().owner, Some(0));
    assert_eq!(st.cell(d).unwrap().owner, None, "impassable is never owned");
}

#[test]
fn a_region_that_reaches_the_board_edge_is_not_captured() {
    let mut st = state(3, 16);
    let (c, d, gap, ring) = ringed_pair(&mut st);
    let other = *ring.iter().find(|h| h.distance(gap) > 1).unwrap();
    set(&mut st, other, None, 1);
    close_gap(&mut st, gap, &ring);
    assert_eq!(st.cell(c).unwrap().owner, Some(1), "a gap still leads out");
    assert_eq!(st.cell(d).unwrap().owner, None);
}

#[test]
fn without_region_capture_only_single_owner_encirclement_applies() {
    let mut st = state(3, 16);
    st.settings.rules.region_capture = false;
    let (c, d, gap, ring) = ringed_pair(&mut st);
    close_gap(&mut st, gap, &ring);
    assert_eq!(st.cell(c).unwrap().owner, Some(1), "c borders neutral d");
    assert_eq!(st.cell(d).unwrap().owner, None);
}

#[test]
fn demolish_removes_an_owned_building_and_uses_the_action() {
    let mut st = state(2, 21);
    let h = first_owned(&st, 0);
    put_building(&mut st, h, Building::Wall);
    let troops = st.cell(h).unwrap().troops;
    st.step(&act(0, Action::Demolish { at: h }));
    let c = st.cell(h).unwrap();
    assert_eq!(c.building, None);
    assert_eq!(c.owner, Some(0));
    assert!(c.troops >= troops, "no troops are spent");
    assert!(st.last_events.iter().any(|e| matches!(
        e,
        Event::Demolished { player: 0, at, building: Building::Wall } if *at == h
    )));
    assert!(
        !st.view(0)
            .visible_events
            .iter()
            .any(|e| matches!(e, Event::Demolished { .. })),
        "bots frozen before v0.7 cannot parse it"
    );
    st.step(&act(0, Action::Demolish { at: h }));
    assert!(st.last_events.iter().any(|e| matches!(
        e,
        Event::Rejected { player: 0, reason } if reason == "nothing to demolish"
    )));
    let theirs = first_owned(&st, 1);
    put_building(&mut st, theirs, Building::Farm);
    st.step(&act(0, Action::Demolish { at: theirs }));
    assert_eq!(st.cell(theirs).unwrap().building, Some(Building::Farm));
}

// ---------- buildings ----------

#[test]
fn build_costs_and_quarry_discounts_match_spec_examples() {
    let r = RulesConfig::default();
    let v = r.build_cost(Building::Village, true);
    assert_eq!((v.troops, v.resource), (72, 0));
    let w = r.build_cost(Building::Wall, true);
    assert_eq!((w.troops, w.resource), (20, 0));
    let f = r.build_cost(Building::Farm, true);
    assert_eq!((f.troops, f.resource), (18, 0));
    assert_eq!(r.build_cost(Building::Village, false).troops, 120);
    assert_eq!(r.build_cost(Building::Farm, false).troops, 30);
    for b in Building::ALL {
        assert_eq!(r.base_cost(b).1, 0, "{b:?} costs troops only");
    }
}

fn stormy() -> RulesConfig {
    RulesConfig {
        storm_start: 3000,
        storm_end: 9000,
        storm_decay_pct: 25,
        ..RulesConfig::default()
    }
}

#[test]
fn storm_is_off_by_default() {
    assert_eq!(RulesConfig::default().storm_radius(9000, 21, 0), None);
}

#[test]
fn storm_closes_from_the_edge_to_the_innermost_passable_ring() {
    let r = stormy();
    assert_eq!(r.storm_radius(3000, 21, 0), None);
    assert_eq!(r.storm_radius(3001, 21, 0), None, "rounds up, so the edge holds a moment");
    assert_eq!(r.storm_radius(6000, 21, 0), Some(11));
    assert_eq!(r.storm_radius(9000, 21, 2), Some(2));
    assert_eq!(r.storm_radius(9999, 21, 2), Some(2));
}

#[test]
fn storm_drains_troops_then_turns_the_hex_neutral() {
    let mut st = state(2, 20);
    st.settings.rules = stormy();
    clear_owners(&mut st);
    let edge = st
        .board
        .hexes()
        .into_iter()
        .find(|&h| h.distance(Hex::new(0, 0)) == st.board.radius && passable(&st.board, &st.terrain, h))
        .unwrap();
    let safe: Vec<Hex> = st
        .board
        .hexes()
        .into_iter()
        .filter(|&h| h.distance(Hex::new(0, 0)) <= st.storm_floor + 1 && passable(&st.board, &st.terrain, h))
        .collect();
    let (inner, keep) = (safe[0], safe[1]);
    set(&mut st, edge, Some(0), 8);
    set(&mut st, inner, Some(1), 3);
    set(&mut st, keep, Some(0), 3);
    st.tick = 6000;
    assert!(st.in_storm(edge) && !st.in_storm(inner));
    st.step(&noop_all(2));
    assert_eq!(st.cell(edge).unwrap().troops, 6, "8 loses 25% (2), no income");
    assert_eq!(st.cell(inner).unwrap().troops, 4, "inside the zone still earns");
    for _ in 0..6 {
        st.step(&noop_all(2));
    }
    assert_eq!(st.cell(edge).unwrap().owner, None);
}

#[test]
fn every_adjacent_farm_boosts_a_village_without_links() {
    let mut st = state(2, 21);
    clear_owners(&mut st);
    let v = interior_open(&st);
    set(&mut st, v, Some(0), 1);
    put_building(&mut st, v, Building::Village);
    let f = faraway(&st, v);
    set(&mut st, f, Some(1), 3);
    st.tick = 1;
    assert_eq!(st.income(v), 2);
    for (i, d) in HEX_DIRECTIONS.iter().take(3).enumerate() {
        let farm = v.add(*d);
        set(&mut st, farm, Some(0), 1);
        put_building(&mut st, farm, Building::Farm);
        assert_eq!(st.income(v), 3 + i as u32, "{} farms", i + 1);
    }
    let farm = v.add(HEX_DIRECTIONS[0]);
    assert_eq!(st.building_links(farm), vec![v]);
    assert!(st
        .validate_management(
            0,
            &ManagementCommand::SetFarmLink { farm, village: v, enabled: true }
        )
        .is_err());
}

#[test]
fn an_adjacent_quarry_boosts_a_village_alongside_its_farms() {
    let mut st = state(2, 21);
    clear_owners(&mut st);
    let v = interior_open(&st);
    set(&mut st, v, Some(0), 1);
    put_building(&mut st, v, Building::Village);
    let f = faraway(&st, v);
    set(&mut st, f, Some(1), 3);
    st.tick = 1;
    let quarry = v.add(HEX_DIRECTIONS[0]);
    set(&mut st, quarry, Some(0), 1);
    put_building(&mut st, quarry, Building::Quarry);
    assert_eq!(st.income(v), 3);
    assert_eq!(st.building_links(quarry), vec![v]);
    let farm = v.add(HEX_DIRECTIONS[1]);
    set(&mut st, farm, Some(0), 1);
    put_building(&mut st, farm, Building::Farm);
    assert_eq!(st.income(v), 4);
    set(&mut st, quarry, Some(1), 1);
    assert_eq!(st.income(v), 3, "an enemy Quarry boosts nothing");
    st.settings.rules.quarry.village_bonus = 0;
    set(&mut st, quarry, Some(0), 1);
    assert_eq!(st.income(v), 3, "rules without the Quarry bonus");
}

#[test]
fn overtime_raises_attacker_odds_up_to_the_cap() {
    let r = RulesConfig::default();
    assert_eq!(r.attacker_win_milli(0), 470);
    assert_eq!(r.attacker_win_milli(3000), 470);
    assert_eq!(r.attacker_win_milli(4000), 530);
    assert_eq!(r.attacker_win_milli(8000), 770);
    assert_eq!(r.attacker_win_milli(20_000), 850);
    let off = RulesConfig {
        overtime_win_milli_per_1000: 0,
        ..RulesConfig::default()
    };
    assert_eq!(off.attacker_win_milli(9000), 470);
}

#[test]
fn build_rules_terrain_spacing_and_leave_one_troop() {
    let mut st = state(2, 17);
    clear_owners(&mut st);
    let c = interior_open(&st);
    set(&mut st, c, Some(0), 29);
    let f = faraway(&st, c);
    set(&mut st, f, Some(1), 3);
    set_height(&mut st, c, 1);
    st.step(&act(
        0,
        Action::Build {
            at: c,
            building: Building::Farm,
        },
    ));
    assert!(
        st.cell(c).unwrap().building.is_none(),
        "no farms on hills"
    );
    assert!(RulesConfig::default().terrain_allowed(Building::Farm, 0), "plains are fine");
    set_height(&mut st, c, -1);
    st.step(&act(
        0,
        Action::Build {
            at: c,
            building: Building::Farm,
        },
    ));
    assert!(
        st.cell(c).unwrap().building.is_none(),
        "30 troops on hex is not enough: one must remain"
    );
    st.step(&act(
        0,
        Action::Build {
            at: c,
            building: Building::Farm,
        },
    ));
    assert_eq!(st.cell(c).unwrap().building, Some(Building::Farm));
    assert_eq!(st.cell(c).unwrap().troops, 32 - 30);
}

#[test]
fn farm_alone_makes_only_base_income_and_no_resource() {
    let mut st = state(2, 18);
    clear_owners(&mut st);
    let c = interior_open(&st);
    set(&mut st, c, Some(0), 100);
    set_height(&mut st, c, -2);
    let f = faraway(&st, c);
    set(&mut st, f, Some(1), 3);
    st.step(&act(
        0,
        Action::Build {
            at: c,
            building: Building::Farm,
        },
    ));
    let after_build = st.cell(c).unwrap().troops;
    for _ in 0..10 {
        st.step(&noop_all(2));
    }
    assert_eq!(st.cell(c).unwrap().troops, after_build + 10);
    assert_eq!(st.resources[0], 0);
}

#[test]
fn linked_farm_rules_boost_only_the_linked_village() {
    let mut st = state(2, 19);
    st.settings.rules.farm.adjacent_boost = false;
    clear_owners(&mut st);
    let c = interior_open(&st);
    let (village, farm) = (c, c.add(HEX_DIRECTIONS[0]));
    set(&mut st, village, Some(0), 1);
    set(&mut st, farm, Some(0), 1);
    let f = faraway(&st, c);
    set(&mut st, f, Some(1), 3);
    put_building(&mut st, village, Building::Village);
    put_building(&mut st, farm, Building::Farm);
    st.tick = 1;
    assert_eq!(st.income(village), 2);
    let link = PlayerCommand {
        management: vec![ManagementCommand::SetFarmLink {
            farm,
            village,
            enabled: true,
        }],
        action: Action::NoOp,
    };
    st.step(&[(0, link)]);
    assert_eq!(st.income(village), 3);
    assert_eq!(st.income(farm), 1);
}

#[test]
fn quarry_requires_hill_and_spacing() {
    let mut st = state(2, 20);
    clear_owners(&mut st);
    let c = interior_open(&st);
    let other = Hex::new(c.q + 1, c.r);
    for h in [c, other] {
        set(&mut st, h, Some(0), 400);
        set_height(&mut st, h, 1);
    }
    let f = faraway(&st, c);
    set(&mut st, f, Some(1), 3);
    st.step(&act(
        0,
        Action::Build {
            at: c,
            building: Building::Quarry,
        },
    ));
    assert_eq!(st.cell(c).unwrap().building, Some(Building::Quarry));
    st.step(&act(
        0,
        Action::Build {
            at: other,
            building: Building::Quarry,
        },
    ));
    assert!(
        st.cell(other).unwrap().building.is_none(),
        "needs 3 hexes of spacing"
    );
}

// ---------- fog ----------

fn scramble_hidden(st: &mut State, viewer: PlayerId) {
    let mask = st.visible_mask(viewer);
    for h in st.board.hexes() {
        let i = st.board.index(h).unwrap();
        if !mask[i] && st.cells[i].owner != Some(viewer) {
            st.cells[i].troops = st.cells[i].troops.wrapping_mul(7).wrapping_add(13);
            st.cells[i].building = Some(Building::Wall);
            st.cells[i].owner = if st.cells[i].owner.is_some() {
                None
            } else {
                Some(1)
            };
        }
    }
    let far = st
        .board
        .hexes()
        .into_iter()
        .find(|&h| !mask[st.board.index(h).unwrap()])
        .unwrap();
    st.last_events.push(Event::Moved {
        player: 1,
        from: far,
        to: far,
        troops: 99,
    });
    st.resources[1] += 40;
}

#[test]
fn fog_leak_hidden_state_does_not_change_view() {
    let a = state(4, 21);
    let mut b = a.clone();
    scramble_hidden(&mut b, 0);
    assert_eq!(
        serde_json::to_vec(&a.view(0)).unwrap(),
        serde_json::to_vec(&b.view(0)).unwrap()
    );
}

#[test]
fn fog_leak_hidden_actions_do_not_change_view() {
    let mut a = state(4, 33);
    let mut b = a.clone();
    let far = first_owned(&a, 3);
    assert!(far.distance(first_owned(&a, 0)) > 4 + a.rules().fog_radius);
    let to = open_neighbor(&a, far);
    a.step(&noop_all(4));
    let mut cmds = noop_all(4);
    cmds[3].1.action = Action::Move {
        from: far,
        to,
        troops: 5,
    };
    b.step(&cmds);
    assert_eq!(
        serde_json::to_vec(&a.view(0)).unwrap(),
        serde_json::to_vec(&b.view(0)).unwrap()
    );
    assert_ne!(a.state_hash(), b.state_hash());
}

#[test]
fn view_omits_hidden_mutable_state_entirely() {
    let st = state(4, 21);
    let v = st.view(0);
    let own = first_owned(&st, 0);
    assert!(v
        .visible_hexes
        .iter()
        .all(|c| c.hex.distance(own) <= st.rules().fog_radius));
    let json = serde_json::to_string(&v).unwrap();
    assert!(!json.contains("\"hidden\"") && !json.contains("redacted"));
    assert!(v
        .public_players
        .iter()
        .all(|p| p.alive && p.placement.is_none()));
    assert!(v
        .visible_events
        .iter()
        .all(|e| !matches!(e, Event::Rejected { player, .. } if *player != 0)));
}

// ---------- elimination, placement, replay ----------

fn scripted(st: &State, p: PlayerId) -> PlayerCommand {
    let mut best: Option<(Hex, Hex, u32)> = None;
    for h in st.board.hexes() {
        let c = st.cell(h).unwrap();
        if c.owner != Some(p) || c.troops < 8 {
            continue;
        }
        if let Some(t) = h
            .neighbors()
            .into_iter()
            .find(|&n| passable(&st.board, &st.terrain, n) && st.cell(n).unwrap().owner != Some(p))
        {
            if best.map_or(true, |b| c.troops > b.2) {
                best = Some((h, t, c.troops));
            }
        }
    }
    PlayerCommand::action(
        best.map(|(from, to, t)| Action::Move {
            from,
            to,
            troops: t - 1,
        })
        .unwrap_or(Action::NoOp),
    )
}

fn run_scripted(seed: u64, ticks: u32) -> (State, Vec<TickCommands>, Vec<Vec<Event>>) {
    let mut st = state(4, seed);
    let (mut log, mut ev) = (vec![], vec![]);
    for _ in 0..ticks {
        if st.is_over() {
            break;
        }
        let cmds: TickCommands = (0..4u16)
            .filter(|&p| !st.eliminated[p as usize])
            .map(|p| (p, scripted(&st, p)))
            .collect();
        st.step(&cmds);
        log.push(cmds);
        ev.push(st.last_events.clone());
    }
    (st, log, ev)
}

#[test]
fn elimination_order_and_results_are_deterministic() {
    let (a, ..) = run_scripted(5, 400);
    let (b, ..) = run_scripted(5, 400);
    assert_eq!(a.eliminated_at, b.eliminated_at);
    assert_eq!(a.final_result(), b.final_result());
}

#[test]
fn eliminated_players_get_public_placement() {
    let mut st = state(3, 22);
    clear_owners(&mut st);
    let c = interior_open(&st);
    set(&mut st, c, Some(1), 1);
    set(&mut st, c.add(HEX_DIRECTIONS[0]), Some(0), 50);
    let f = faraway(&st, c);
    set(&mut st, f, Some(2), 10);
    set_height(&mut st, c, 0);
    set_height(&mut st, c.add(HEX_DIRECTIONS[0]), 0);
    st.step(&act(
        0,
        Action::Move {
            from: c.add(HEX_DIRECTIONS[0]),
            to: c,
            troops: 45,
        },
    ));
    assert!(st.eliminated[1]);
    assert_eq!(st.elim_place[1], Some((3, 3)));
    let v = st.view(2);
    assert_eq!(v.public_players[1].placement, Some((3, 3)));
}

#[test]
fn replay_produces_identical_checksum() {
    let (st, log, ev) = run_scripted(11, 120);
    let replay = build_replay(&st, log, ev);
    assert_eq!(replay_state(&replay).unwrap().state_hash(), st.state_hash());
    assert_eq!(replay.checksum, format!("{:016x}", st.state_hash()));
    let json = serde_json::to_string(&replay).unwrap();
    let back: Replay = serde_json::from_str(&json).unwrap();
    assert_eq!(replay_state(&back).unwrap().state_hash(), st.state_hash());
    let mut tampered = back.clone();
    tampered.rules.base_income = 2;
    assert!(replay_state(&tampered).is_err());
}

#[test]
fn rules_hash_is_stable_and_sensitive() {
    let r = RulesConfig::default();
    assert_eq!(r.hash(), RulesConfig::default().hash());
    let mut r2 = r.clone();
    r2.wall.troop_cost += 1;
    assert_ne!(r.hash(), r2.hash());
}

/// Player 0 eliminates player 1 on an otherwise unowned board.
fn last_one_standing(rules: RulesConfig) -> State {
    let mut s = settings(2, 24);
    s.rules = rules;
    let mut st = State::new(s, vec![]).unwrap();
    clear_owners(&mut st);
    let c = interior_open(&st);
    let from = c.add(HEX_DIRECTIONS[0]);
    set(&mut st, c, Some(1), 1);
    set(&mut st, from, Some(0), 50);
    set_height(&mut st, c, 0);
    set_height(&mut st, from, 0);
    st.step(&act(0, Action::Move { from, to: c, troops: 45 }));
    assert!(st.eliminated[1]);
    st
}

#[test]
fn v0_1_rules_continue_until_one_player_owns_all_land() {
    let rules = RulesConfig {
        version: "v0.1".into(),
        end_on_total_conquest: true,
        ..RulesConfig::default()
    };
    let mut st = last_one_standing(rules);
    assert!(!st.is_over(), "neutral land is still unclaimed");
    for h in st.board.hexes() {
        if passable(&st.board, &st.terrain, h) {
            set(&mut st, h, Some(0), 1);
        }
    }
    st.step(&noop_all(2));
    let r = st.result.clone().expect("total conquest ends the match");
    assert!(r.final_tick < RulesConfig::default().max_ticks);
    assert_eq!(r.placements[0].player, 0);
}

#[test]
fn match_ends_when_one_player_is_left_standing() {
    let st = last_one_standing(RulesConfig::default());
    let r = st.result.clone().expect("the last player standing ends the match");
    assert!(r.final_tick < RulesConfig::default().max_ticks);
    assert_eq!(r.placements[0].player, 0);
}

#[test]
fn default_tick_limit_is_10000() {
    assert_eq!(RulesConfig::default().max_ticks, 10_000);
}

#[test]
fn tick_limit_ranks_survivors() {
    let mut s = settings(2, 23);
    s.rules.max_ticks = 3;
    let mut st = State::new(s, vec![]).unwrap();
    for _ in 0..3 {
        st.step(&noop_all(2));
    }
    let r = st.result.clone().unwrap();
    assert_eq!(r.final_tick, 3);
    assert!((0..2).all(|p| r.placement(p).eliminated_at.is_none()));
}

fn real_map(preset: MapPreset, players: u16, seed: u64) -> MatchSettings {
    let mut s = settings(players, seed);
    s.rules.map = Some(preset);
    s
}

fn open_components(st: &State) -> usize {
    let mut seen = vec![false; st.board.slots()];
    let mut parts = 0;
    for h in st.board.hexes() {
        let i = st.board.index(h).unwrap();
        if seen[i] || !passable(&st.board, &st.terrain, h) {
            continue;
        }
        parts += 1;
        let mut stack = vec![h];
        seen[i] = true;
        while let Some(c) = stack.pop() {
            for nb in c.neighbors() {
                if let Some(j) = st.board.index(nb) {
                    if !seen[j] && passable(&st.board, &st.terrain, nb) {
                        seen[j] = true;
                        stack.push(nb);
                    }
                }
            }
        }
    }
    parts
}

#[test]
fn every_real_map_is_one_landmass_with_a_start_for_every_lobby_size() {
    for preset in atlas::PRESETS {
        for players in [2u16, 3, 4, 6, 8] {
            for seed in 0..2 {
                let st = State::new(real_map(preset, players, seed), vec![]).unwrap();
                let at = format!("{preset:?}, {players} players, seed {seed}");
                assert_eq!(open_components(&st), 1, "{at}");
                let owned = (0..players).filter(|&p| st.hex_counts()[p as usize] == 1).count();
                assert_eq!(owned, players as usize, "{at}");
                let open = st.terrain.iter().flatten().filter(|t| t.passable()).count();
                let water = st.terrain.iter().flatten().filter(|t| t.kind == TerrainKind::Water).count();
                assert!(water * 6 > st.board.hexes().len(), "{at}: the sea should be a real part of the board");
                assert!(open >= 400, "{at}: only {open} open hexes");
            }
        }
    }
}

#[test]
fn real_maps_have_farmland_and_quarry_hills() {
    for preset in atlas::PRESETS {
        let st = State::new(real_map(preset, 6, 9), vec![]).unwrap();
        let open: Vec<i8> = st.terrain.iter().flatten().filter(|t| t.passable()).map(|t| t.height).collect();
        assert!(open.iter().filter(|&&h| h <= 0).count() * 2 > open.len(), "{preset:?}");
        assert!(open.iter().filter(|&&h| h >= 1).count() * 5 > open.len(), "{preset:?}");
    }
}

#[test]
fn bots_are_not_told_which_real_map_they_are_on() {
    let st = State::new(real_map(MapPreset::Japan, 2, 1), vec![]).unwrap();
    assert_eq!(st.view(0).rules.unwrap().map, None);
}

fn print_map(st: &State) {
    let r = st.board.radius;
    for row in -r..=r {
        let mut line = " ".repeat(row.unsigned_abs() as usize);
        for q in -r..=r {
            let h = Hex::new(q, row);
            let Some(t) = st.board.index(h).and_then(|i| st.terrain[i]) else { continue };
            let c = match (t.kind, st.cell(h).unwrap().owner) {
                (_, Some(_)) => '@',
                (TerrainKind::Water, _) => '.',
                (TerrainKind::Mountain, _) => '^',
                (TerrainKind::Open, _) => match t.height { -2 => '_', -1 => '-', 0 => '=', 1 => 'n', _ => 'N' },
            };
            line.push(c);
            line.push(' ');
        }
        println!("{line}");
    }
}

/// `ATLAS_ASCII=japan cargo test -p hexarena-engine real_map_cities -- --nocapture` draws a map.
#[test]
fn real_map_cities_land_where_they_should() {
    let x = |h: Hex| h.q as f64 + h.r as f64 / 2.0;
    let checks: &[(MapPreset, &str, &str, &str, &str)] = &[
        // (preset, west, east, north, south)
        (MapPreset::Europe, "Madrid", "Berlin", "Paris", "Rome"),
        (MapPreset::Mediterranean, "Madrid", "Istanbul", "Rome", "Tripoli"),
        (MapPreset::Japan, "Fukuoka", "Tokyo", "Sapporo", "Kagoshima"),
        (MapPreset::China, "Kashgar", "Shanghai", "Harbin", "Guangzhou"),
        (MapPreset::UnitedStates, "Seattle", "New York", "Minneapolis", "Houston"),
        (MapPreset::NorthAmerica, "Los Angeles", "Montreal", "Edmonton", "Mexico City"),
        (MapPreset::Africa, "Dakar", "Mogadishu", "Cairo", "Cape Town"),
    ];
    for &(preset, west, east, north, south) in checks {
        let st = State::new(real_map(preset, 4, 1), vec![]).unwrap();
        let places = atlas::places(preset, st.board.radius);
        let at = |name: &str| places.iter().find(|p| p.0 == name).unwrap_or_else(|| panic!("{name} on {preset:?}")).1;
        assert!(x(at(west)) < x(at(east)), "{preset:?}: {west} west of {east}");
        assert!(at(north).r < at(south).r, "{preset:?}: {north} north of {south}");
        let wet: Vec<&str> = places
            .iter()
            .filter(|(_, h)| st.board.index(*h).and_then(|i| st.terrain[i]).unwrap().kind == TerrainKind::Water)
            .map(|p| p.0)
            .collect();
        assert!(wet.len() * 5 <= places.len(), "{preset:?}: at sea {wet:?}");
        if std::env::var("ATLAS_ASCII").is_ok_and(|v| serde_json::to_string(&preset).unwrap() == format!("\"{v}\"")) {
            println!("{preset:?}, radius {}", st.board.radius);
            print_map(&st);
        }
    }
}
