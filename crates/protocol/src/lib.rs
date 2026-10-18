//! Versioned public types shared by the engine, bots, server, and replay viewer.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

pub type PlayerId = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hex {
    pub q: i32,
    pub r: i32,
}

pub const HEX_DIRECTIONS: [Hex; 6] = [
    Hex { q: 1, r: 0 },
    Hex { q: 1, r: -1 },
    Hex { q: 0, r: -1 },
    Hex { q: -1, r: 0 },
    Hex { q: -1, r: 1 },
    Hex { q: 0, r: 1 },
];

impl Hex {
    pub const fn new(q: i32, r: i32) -> Self {
        Hex { q, r }
    }
    pub fn add(self, o: Hex) -> Hex {
        Hex::new(self.q + o.q, self.r + o.r)
    }
    pub fn distance(self, o: Hex) -> i32 {
        let dq = self.q - o.q;
        let dr = self.r - o.r;
        (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
    }
    pub fn neighbors(self) -> [Hex; 6] {
        HEX_DIRECTIONS.map(|d| self.add(d))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainKind {
    Open,
    /// Impassable high ground.
    Mountain,
    /// Impassable low ground.
    Water,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terrain {
    /// -2 ..= +2
    pub height: i8,
    pub kind: TerrainKind,
}

impl Terrain {
    pub fn passable(&self) -> bool {
        self.kind == TerrainKind::Open
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Building {
    /// Produces resources; linked to Villages within range.
    Farm,
    /// Hub for Farm links.
    Village,
    /// Resource site; may be pre-placed by the map generator.
    Quarry,
    /// Raises defense; connected adjacent Walls form visible lines.
    Wall,
}

impl Building {
    pub const ALL: [Building; 4] = [
        Building::Farm,
        Building::Village,
        Building::Quarry,
        Building::Wall,
    ];
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Pass,
    /// Move `troops` from an owned hex to an adjacent hex.
    Move {
        from: Hex,
        to: Hex,
        troops: u32,
    },
    Build {
        at: Hex,
        building: Building,
    },
}

/// All balance numbers. PROVISIONAL. Versioned; immutable once public matches use a version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RulesConfig {
    pub rules_version: String,
    pub min_players: u16,
    pub max_players: u16,
    pub radius_per_sqrt_player: f64,
    pub impassable_target_pct: u32,
    pub impassable_min_pct: u32,
    pub impassable_max_pct: u32,
    pub vision_radius: i32,
    pub public_terrain: bool,
    pub income_per_hex: u32,
    /// Defender strength percent indexed by (height + 2).
    pub defense_pct_by_height: [u32; 5],
    pub wall_defense_bonus_pct: u32,
    pub start_troops: u32,
    pub garrison_min: u32,
    pub garrison_max: u32,
    pub quarries_per_player: u32,
    pub quarry_income: u32,
    pub farm_village_link_range: i32,
    pub farm_link_income: u32,
    pub build_cost: [u32; 4],
    pub tick_limit: u32,
    pub spawn_min_distance_factor: f64,
}

impl Default for RulesConfig {
    fn default() -> Self {
        RulesConfig {
            rules_version: "alpha-1".into(),
            min_players: 2,
            max_players: 8,
            radius_per_sqrt_player: 7.5,
            impassable_target_pct: 20,
            impassable_min_pct: 15,
            impassable_max_pct: 25,
            vision_radius: 3,
            public_terrain: true,
            income_per_hex: 1,
            defense_pct_by_height: [60, 80, 100, 130, 170],
            wall_defense_bonus_pct: 40,
            start_troops: 10,
            garrison_min: 2,
            garrison_max: 14,
            quarries_per_player: 2,
            quarry_income: 2,
            farm_village_link_range: 4,
            farm_link_income: 1,
            build_cost: [8, 12, 10, 6],
            tick_limit: 1500,
            spawn_min_distance_factor: 0.6,
        }
    }
}

impl RulesConfig {
    pub fn radius_for_players(&self, players: u16) -> i32 {
        (self.radius_per_sqrt_player * (players as f64).sqrt()).round() as i32
    }
    pub fn cost(&self, b: Building) -> u32 {
        self.build_cost[b as usize]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchSettings {
    pub players: u16,
    pub map_seed: u64,
    pub spawn_seed: u64,
    pub rules: RulesConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleCell {
    pub hex: Hex,
    pub owner: Option<PlayerId>,
    pub troops: u32,
    pub building: Option<Building>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Moved {
        player: PlayerId,
        from: Hex,
        to: Hex,
        troops: u32,
    },
    Captured {
        player: PlayerId,
        at: Hex,
        previous_owner: Option<PlayerId>,
        troops_left: u32,
    },
    Repelled {
        attacker: PlayerId,
        at: Hex,
        attackers_lost: u32,
    },
    Built {
        player: PlayerId,
        at: Hex,
        building: Building,
    },
    Eliminated {
        player: PlayerId,
    },
    /// Visible only to the player whose action was rejected.
    Rejected {
        player: PlayerId,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicPlayer {
    pub id: PlayerId,
    pub name: String,
    pub eliminated: bool,
}

/// Everything a player (human or CPU) is allowed to know. Fog-filtered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub protocol_version: u32,
    pub rules_version: String,
    pub tick: u32,
    pub player: PlayerId,
    pub radius: i32,
    pub vision_radius: i32,
    pub players: Vec<PublicPlayer>,
    /// Row-major over the (2R+1)^2 axial bounding grid; `None` for off-board or (if
    /// `public_terrain` is off) unseen cells.
    pub terrain: Vec<Option<Terrain>>,
    /// Visible cells, sorted by (r, q).
    pub visible: Vec<VisibleCell>,
    pub own_resources: u32,
    pub events: Vec<Event>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchResult {
    /// Set only when a single player finished strictly first.
    pub winner: Option<PlayerId>,
    pub ticks: u32,
    pub final_hexes: Vec<u32>,
    /// Groups of players, best finish first; players in a group are tied.
    pub placement_order: Vec<Vec<PlayerId>>,
    /// Per player, 1-based; ties share the average of the places they span (e.g. 3.5).
    pub places: Vec<f64>,
    /// Per player: N - place + 1.
    pub points: Vec<f64>,
    /// Per player: (N - place) / (N - 1).
    pub normalized: Vec<f64>,
    /// Per player: tick of elimination, or the final tick for survivors.
    pub survival_ticks: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replay {
    pub protocol_version: u32,
    pub settings: MatchSettings,
    pub names: Vec<String>,
    /// Per tick, the (player, action) pairs submitted.
    pub actions: Vec<Vec<(PlayerId, Action)>>,
    pub result: MatchResult,
    pub final_state_hash: u64,
}
