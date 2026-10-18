use super::*;
use mapgen::{generate, has_two_independent_routes};

fn settings(players: u16, seed: u64) -> MatchSettings {
    MatchSettings {
        players,
        map_seed: seed,
        spawn_seed: seed ^ 0xFEED,
        rules: RulesConfig::default(),
    }
}

fn pass_all(n: u16) -> Vec<(PlayerId, Action)> {
    (0..n).map(|p| (p, Action::Pass)).collect()
}

#[test]
fn hex_count_formula() {
    for r in 1..25 {
        assert_eq!(Board::new(r).hexes().len() as i32, 1 + 3 * r * (r + 1));
    }
}

#[test]
fn radius_scaling() {
    let r = RulesConfig::default();
    assert_eq!(r.radius_for_players(4), 15);
    assert_eq!(r.radius_for_players(8), 21);
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
fn map_safety_invariants() {
    for players in [2u16, 3, 4, 6, 8] {
        for seed in 0..6u64 {
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
            // single connected component
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
fn eight_player_map_scale() {
    let m = generate(&settings(8, 42)).unwrap();
    let board = Board::new(m.radius);
    let total = board.hexes().len();
    let open = board
        .hexes()
        .iter()
        .filter(|&&h| passable(&board, &m.terrain, h))
        .count();
    assert!((1300..=1500).contains(&total), "{total}");
    assert!((1000..=1250).contains(&open), "{open}");
}

#[test]
fn spawns_are_not_symmetric_in_value() {
    let m = generate(&settings(4, 9)).unwrap();
    let board = Board::new(m.radius);
    let vals: Vec<u32> = m
        .spawns
        .iter()
        .map(|sp| {
            board
                .hexes()
                .iter()
                .filter(|h| h.distance(*sp) <= 4)
                .map(|h| m.garrison[board.index(*h).unwrap()])
                .sum()
        })
        .collect();
    assert!(vals.iter().any(|v| *v != vals[0]), "{vals:?}");
}

#[test]
fn income_is_one_per_owned_hex_regardless_of_height() {
    let mut st = State::new(settings(2, 3), vec![]).unwrap();
    let sp = st
        .board
        .hexes()
        .into_iter()
        .find(|&h| st.cell(h).unwrap().owner == Some(0))
        .unwrap();
    let before = st.cell(sp).unwrap().troops;
    st.step(&pass_all(2));
    assert_eq!(st.cell(sp).unwrap().troops, before + 1);
}

fn first_owned(st: &State, p: PlayerId) -> Hex {
    st.board
        .hexes()
        .into_iter()
        .find(|&h| st.cell(h).unwrap().owner == Some(p))
        .unwrap()
}

#[test]
fn rejects_impassable_and_overdraw() {
    let mut st = State::new(settings(2, 5), vec![]).unwrap();
    let sp = first_owned(&st, 0);
    let troops = st.cell(sp).unwrap().troops;
    st.step(&[(
        0,
        Action::Move {
            from: sp,
            to: sp.neighbors()[0],
            troops,
        },
    )]);
    assert!(st
        .last_events
        .iter()
        .any(|e| matches!(e, Event::Rejected { player: 0, .. })));
    let blocked = sp
        .neighbors()
        .into_iter()
        .find(|&n| st.board.contains(n) && !passable(&st.board, &st.terrain, n));
    if let Some(b) = blocked {
        st.step(&[(
            0,
            Action::Move {
                from: sp,
                to: b,
                troops: 1,
            },
        )]);
        assert!(st.cell(b).unwrap().owner.is_none());
    }
}

#[test]
fn height_modulates_conquest_cost() {
    let r = RulesConfig::default();
    assert!(r.defense_pct_by_height[0] < r.defense_pct_by_height[4]);
}

#[test]
fn replay_is_deterministic() {
    let s = settings(3, 11);
    let mut st = State::new(s.clone(), vec![]).unwrap();
    let mut log = vec![];
    for _ in 0..30 {
        let acts: Vec<(PlayerId, Action)> = (0..3u16)
            .filter_map(|p| {
                let from = first_owned(&st, p);
                let c = st.cell(from).unwrap();
                let to = from
                    .neighbors()
                    .into_iter()
                    .find(|&n| passable(&st.board, &st.terrain, n))?;
                (c.troops > 1).then_some((
                    p,
                    Action::Move {
                        from,
                        to,
                        troops: c.troops - 1,
                    },
                ))
            })
            .collect();
        st.step(&acts);
        log.push(acts);
    }
    let replay = Replay {
        protocol_version: PROTOCOL_VERSION,
        settings: s,
        names: vec![],
        actions: log,
        result: st.final_result(),
        final_state_hash: st.state_hash(),
    };
    assert_eq!(
        replay_state(&replay).unwrap().state_hash(),
        replay.final_state_hash
    );
}

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
    st.last_events.push(Event::Moved {
        player: 1,
        from: Hex::new(0, 0),
        to: Hex::new(0, 0),
        troops: 99,
    });
}

#[test]
fn fog_leak_hidden_state_does_not_change_view() {
    let a = State::new(settings(4, 21), vec![]).unwrap();
    let mut b = a.clone();
    let mask = a.visible_mask(0);
    // ensure the scrambled hex set excludes the viewer's vision, including that event hex
    let ev_hex_visible = mask[a.board.index(Hex::new(0, 0)).unwrap()];
    scramble_hidden(&mut b, 0);
    if ev_hex_visible {
        b.last_events.pop();
    }
    assert_eq!(
        serde_json::to_vec(&a.view(0)).unwrap(),
        serde_json::to_vec(&b.view(0)).unwrap()
    );
}

#[test]
fn fog_leak_hidden_actions_do_not_change_view() {
    let mut a = State::new(settings(4, 33), vec![]).unwrap();
    let mut b = a.clone();
    let far = first_owned(&a, 3);
    assert!(far.distance(first_owned(&a, 0)) > 4 + a.rules().vision_radius);
    let to = far
        .neighbors()
        .into_iter()
        .find(|&n| passable(&a.board, &a.terrain, n))
        .unwrap();
    a.step(&pass_all(4));
    let mut acts = pass_all(4);
    acts[3].1 = Action::Move {
        from: far,
        to,
        troops: 5,
    };
    b.step(&acts);
    assert_eq!(
        serde_json::to_vec(&a.view(0)).unwrap(),
        serde_json::to_vec(&b.view(0)).unwrap()
    );
    assert_ne!(a.state_hash(), b.state_hash());
}

#[test]
fn view_only_contains_visible_cells() {
    let st = State::new(settings(4, 21), vec![]).unwrap();
    let v = st.view(0);
    let own = first_owned(&st, 0);
    assert!(v.visible.iter().all(|c| c.hex.distance(own) <= 3));
    assert!(v
        .events
        .iter()
        .all(|e| !matches!(e, Event::Rejected { player, .. } if *player != 0)));
}
