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

/// Fixed-point (milli) arithmetic is used everywhere a rule needs a fraction so replays
/// never diverge across platforms.
pub const MILLI: u32 = 1000;

/// Points are stored in milli-points (8 points = 8000).
pub type FixedScore = u32;
pub type Tick = u32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    NoOp,
    /// Move `troops` from an owned hex to an adjacent hex.
    Move {
        from: Hex,
        to: Hex,
        troops: u32,
    },
    /// Every owned hex with spare troops steps one hex toward `target` through friendly land.
    Gather {
        target: Hex,
    },
    /// Construct a building. Paid in troops from that hex plus global Resource.
    Build {
        at: Hex,
        building: Building,
    },
    /// Tear down the building on an owned hex. Nothing is refunded.
    Demolish {
        at: Hex,
    },
}

/// Free management commands. They never consume the main action but have cooldowns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManagementCommand {
    SetFarmLink {
        farm: Hex,
        village: Hex,
        enabled: bool,
    },
    SetWallLink {
        wall_a: Hex,
        wall_b: Hex,
        enabled: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlayerCommand {
    pub management: Vec<ManagementCommand>,
    pub action: Action,
}

impl Default for Action {
    fn default() -> Self {
        Action::NoOp
    }
}

impl PlayerCommand {
    pub fn action(action: Action) -> Self {
        PlayerCommand {
            management: vec![],
            action,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FarmRules {
    pub troop_cost: u32,
    pub resource_cost: u32,
    pub allowed_heights: Vec<i8>,
    pub sockets: u8,
    pub relink_cooldown: u32,
    /// Every adjacent friendly Farm boosts a Village by `village.farm_bonus`, with no
    /// links or sockets (v0.5 onward). When false, a Farm boosts only the Villages it
    /// links, up to `sockets` of them, one Farm per Village.
    #[serde(default, skip_serializing_if = "is_false")]
    pub adjacent_boost: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VillageRules {
    pub troop_cost: u32,
    pub resource_cost: u32,
    /// Extra troops per tick on its own hex.
    pub bonus: u32,
    /// Additional troops per tick for each Farm supporting it.
    pub farm_bonus: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuarryRules {
    pub troop_cost: u32,
    pub resource_cost: u32,
    pub allowed_heights: Vec<i8>,
    pub min_friendly_distance: i32,
    pub discount_radius: i32,
    pub troop_discount_pct: u32,
    pub resource_discount: u32,
    /// Extra troops per tick for every friendly Village directly adjacent to the Quarry
    /// (v0.6 onward). Quarries stand `min_friendly_distance` apart, so a Village touches
    /// at most one at the default spacing of 3.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub village_bonus: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WallRules {
    pub troop_cost: u32,
    pub resource_cost: u32,
    pub max_links: u8,
    /// Defense multiplier (milli) indexed by active link count.
    pub defense_milli_by_links: Vec<u32>,
    pub relink_cooldown: u32,
}

/// All balance numbers. PROVISIONAL. Immutable once public ranked matches use a version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RulesConfig {
    pub version: String,
    pub min_players: u16,
    pub max_players: u16,
    pub map_radius_factor: f64,
    /// Board radius is `map_radius_factor * players ^ map_radius_exponent`. At 0.5 (up to
    /// v0.5) area grows with the player count, so every lobby size has the same land per
    /// player; lower exponents pack big lobbies tighter.
    #[serde(default = "sqrt_exponent", skip_serializing_if = "is_sqrt_exponent")]
    pub map_radius_exponent: f64,
    pub impassable_min_pct: u32,
    pub impassable_target_pct: u32,
    pub impassable_max_pct: u32,
    pub starting_troops: u32,
    pub neutral_min: u32,
    pub neutral_max: u32,
    pub base_income: u32,
    pub start_resource: u32,
    pub tick_rate_hz: u32,
    /// Hard cutoff; survivors are ranked by land (troops, resource, buildings only break
    /// exact land ties), below them eliminated players with the latest elimination first.
    pub max_ticks: u32,
    /// End only when one player owns every traversable hex (v0.1). When false, the
    /// match ends as soon as one player is left standing (v0 and v0.2 onward).
    #[serde(default, skip_serializing_if = "is_false")]
    pub end_on_total_conquest: bool,
    pub fog_radius: i32,
    pub queue_limit: usize,
    pub public_terrain: bool,
    pub attacker_exchange_win_milli: u32,
    /// Overtime: from this tick on, attackers win each exchange more often...
    #[serde(default, skip_serializing_if = "is_zero")]
    pub overtime_start: u32,
    /// ...by this much per 1000 ticks (milli; 0 turns overtime off)...
    #[serde(default, skip_serializing_if = "is_zero")]
    pub overtime_win_milli_per_1000: u32,
    /// ...up to this chance.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub overtime_max_win_milli: u32,
    /// Storm: from this tick the safe zone (hexes within some distance of the centre)
    /// shrinks from the board edge...
    #[serde(default, skip_serializing_if = "is_zero")]
    pub storm_start: u32,
    /// ...reaching the innermost passable ring at this tick (0 turns the storm off).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub storm_end: u32,
    /// Hexes outside the zone make no troops and lose this share of their troops (at
    /// least one) every tick; an emptied hex turns neutral.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub storm_decay_pct: u32,
    pub height_factor_milli: u32,
    pub maximum_total_defense_milli: u32,
    pub extractor_period: u32,
    /// Resource per extractor every `extractor_period` ticks (0 = troops-only economy).
    pub extractor_output: u32,
    pub farm: FarmRules,
    pub village: VillageRules,
    pub quarry: QuarryRules,
    pub wall: WallRules,
    pub encirclement_enabled: bool,
    pub impassable_breaks_encirclement: bool,
    /// Go-style capture: a region of hexes not owned by the capturer (enemy, neutral or
    /// mixed, impassable hexes allowed inside) that touches neither the board edge nor
    /// anything but the capturer's land becomes the capturer's, troops and all.
    /// Replaces the single-owner encirclement check when on.
    #[serde(default, skip_serializing_if = "is_false")]
    pub region_capture: bool,
    pub spawn_min_distance: i32,
    /// A hand-drawn landmass instead of the procedural map; `None` is procedural.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<MapPreset>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapPreset {
    /// Mainland Europe from Lisbon to Kyiv, without the islands.
    Europe,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn sqrt_exponent() -> f64 {
    0.5
}

fn is_sqrt_exponent(v: &f64) -> bool {
    *v == 0.5
}

impl Default for RulesConfig {
    fn default() -> Self {
        RulesConfig {
            version: "v0.7".into(),
            min_players: 2,
            max_players: 8,
            map_radius_factor: 9.25,
            map_radius_exponent: 0.25,
            impassable_min_pct: 15,
            impassable_target_pct: 20,
            impassable_max_pct: 25,
            starting_troops: 50,
            neutral_min: 6,
            neutral_max: 24,
            base_income: 1,
            start_resource: 0,
            tick_rate_hz: 2,
            max_ticks: 10_000,
            end_on_total_conquest: false,
            fog_radius: 3,
            queue_limit: 1,
            public_terrain: true,
            attacker_exchange_win_milli: 470,
            overtime_start: 3000,
            overtime_win_milli_per_1000: 60,
            overtime_max_win_milli: 850,
            storm_start: 0,
            storm_end: 0,
            storm_decay_pct: 0,
            height_factor_milli: 1500,
            maximum_total_defense_milli: 5000,
            extractor_period: 10,
            extractor_output: 0,
            farm: FarmRules {
                troop_cost: 30,
                resource_cost: 0,
                allowed_heights: vec![-2, -1, 0],
                sockets: 3,
                relink_cooldown: 10,
                adjacent_boost: true,
            },
            village: VillageRules {
                troop_cost: 120,
                resource_cost: 0,
                bonus: 1,
                farm_bonus: 1,
            },
            quarry: QuarryRules {
                troop_cost: 50,
                resource_cost: 0,
                allowed_heights: vec![1, 2],
                min_friendly_distance: 3,
                discount_radius: 1,
                troop_discount_pct: 40,
                resource_discount: 0,
                village_bonus: 1,
            },
            wall: WallRules {
                troop_cost: 33,
                resource_cost: 0,
                max_links: 2,
                defense_milli_by_links: vec![2000, 2500, 3000],
                relink_cooldown: 10,
            },
            encirclement_enabled: true,
            impassable_breaks_encirclement: true,
            region_capture: true,
            spawn_min_distance: 8,
            map: None,
        }
    }
}

/// Troop and Resource cost of a building after any Quarry discount.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildCost {
    pub base_troops: u32,
    pub base_resource: u32,
    pub troops: u32,
    pub resource: u32,
    pub discounted: bool,
}

impl RulesConfig {
    pub fn radius_for_players(&self, players: u16) -> i32 {
        (self.map_radius_factor * (players as f64).powf(self.map_radius_exponent)).round() as i32
    }

    /// Stable 64-bit FNV-1a hash of the serialized rules, hex encoded.
    pub fn hash(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("rules serialize");
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        format!("{h:016x}")
    }

    pub fn base_cost(&self, b: Building) -> (u32, u32) {
        match b {
            Building::Farm => (self.farm.troop_cost, self.farm.resource_cost),
            Building::Village => (self.village.troop_cost, self.village.resource_cost),
            Building::Quarry => (self.quarry.troop_cost, self.quarry.resource_cost),
            Building::Wall => (self.wall.troop_cost, self.wall.resource_cost),
        }
    }

    pub fn build_cost(&self, b: Building, next_to_friendly_quarry: bool) -> BuildCost {
        let (bt, br) = self.base_cost(b);
        let discounted = next_to_friendly_quarry && b != Building::Quarry;
        let (troops, resource) = if discounted {
            (
                (bt * (100 - self.quarry.troop_discount_pct) + 99) / 100,
                br.saturating_sub(self.quarry.resource_discount),
            )
        } else {
            (bt, br)
        };
        BuildCost {
            base_troops: bt,
            base_resource: br,
            troops,
            resource,
            discounted,
        }
    }

    pub fn terrain_allowed(&self, b: Building, height: i8) -> bool {
        match b {
            Building::Farm => self.farm.allowed_heights.contains(&height),
            Building::Quarry => self.quarry.allowed_heights.contains(&height),
            Building::Village | Building::Wall => true,
        }
    }

    pub fn wall_multiplier_milli(&self, active_links: usize) -> u32 {
        let t = &self.wall.defense_milli_by_links;
        t[active_links.min(t.len() - 1)]
    }

    /// height_factor^(defender - attacker), in milli units, with half-up rounding per step.
    pub fn height_multiplier_milli(&self, attacker_height: i8, defender_height: i8) -> u32 {
        let f = self.height_factor_milli as u64;
        let d = defender_height as i32 - attacker_height as i32;
        let mut m: u64 = MILLI as u64;
        for _ in 0..d.abs() {
            m = if d > 0 {
                (m * f + 500) / 1000
            } else {
                (m * 1000 + f / 2) / f
            };
        }
        m as u32
    }

    /// The attacker's chance (milli) of winning one exchange at `tick`: the base chance,
    /// rising in overtime so held borders eventually break and games end.
    pub fn attacker_win_milli(&self, tick: u32) -> u32 {
        let base = self.attacker_exchange_win_milli;
        if self.overtime_win_milli_per_1000 == 0 || tick <= self.overtime_start {
            return base;
        }
        let extra = (tick - self.overtime_start) as u64 * self.overtime_win_milli_per_1000 as u64 / 1000;
        (base as u64 + extra).min(self.overtime_max_win_milli.max(base) as u64) as u32
    }

    /// The storm's safe distance from the centre at `tick` on a board of `radius`, or
    /// `None` while the whole board is safe. It shrinks linearly from `radius` at
    /// `storm_start` to `floor` (the innermost ring holding a passable hex) at `storm_end`.
    pub fn storm_radius(&self, tick: u32, radius: i32, floor: i32) -> Option<i32> {
        if self.storm_end <= self.storm_start || tick <= self.storm_start {
            return None;
        }
        let left = self.storm_end.saturating_sub(tick) as u64;
        let span = (self.storm_end - self.storm_start) as u64;
        let r = floor as i64 + ((radius - floor).max(0) as u64 * left).div_ceil(span) as i64;
        (r < radius as i64).then_some(r as i32)
    }

    /// Terrain multiplied by Wall multiplier, capped.
    pub fn defense_milli(
        &self,
        attacker_height: i8,
        defender_height: i8,
        wall_links: Option<usize>,
    ) -> u32 {
        let h = self.height_multiplier_milli(attacker_height, defender_height) as u64;
        let w = wall_links
            .map(|n| self.wall_multiplier_milli(n))
            .unwrap_or(MILLI) as u64;
        ((h * w / 1000) as u32).min(self.maximum_total_defense_milli)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchSettings {
    pub players: u16,
    pub terrain_seed: u64,
    pub spawn_seed: u64,
    pub match_seed: u64,
    pub rules: RulesConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleBuilding {
    pub kind: Building,
    /// Ticks until the next Resource unit (Farm/Quarry), else 0.
    pub production_remaining: u32,
    /// Farm: supported Villages. Village: `[supporting Farm]`. Wall: active partner Walls.
    pub links: Vec<Hex>,
    /// Ticks until this building's links may be changed again.
    pub link_cooldown: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleHex {
    pub hex: Hex,
    pub owner: Option<PlayerId>,
    pub troops: u32,
    pub building: Option<VisibleBuilding>,
    /// Troops per tick this hex currently produces.
    pub income: u32,
    /// Wall defense multiplier (milli); 1000 without a Wall.
    pub wall_milli: u32,
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
    Gathered {
        player: PlayerId,
        target: Hex,
        sources: u32,
    },
    Captured {
        player: PlayerId,
        at: Hex,
        previous_owner: Option<PlayerId>,
        troops_left: u32,
        building: Option<Building>,
    },
    Repelled {
        attacker: PlayerId,
        at: Hex,
        attackers_lost: u32,
    },
    Surrendered {
        capturer: PlayerId,
        from: PlayerId,
        hexes: Vec<Hex>,
    },
    Built {
        player: PlayerId,
        at: Hex,
        building: Building,
    },
    /// Recorded in replays only: bots frozen before v0.7 cannot parse it, so views
    /// leave it out (the hex's building simply disappears).
    Demolished {
        player: PlayerId,
        at: Hex,
        building: Building,
    },
    LinkChanged {
        player: PlayerId,
        at: Hex,
    },
    /// Links at this hex broke because ownership changed.
    LinksBroken {
        at: Hex,
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
    pub alive: bool,
    /// Known finishing place range once eliminated.
    pub placement: Option<(u16, u16)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerEconomy {
    pub resource: u32,
}

/// Everything a player (human or CPU) is allowed to know. Fog-filtered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub protocol_version: u32,
    pub rules_version: String,
    pub tick: Tick,
    pub you: PlayerId,
    pub radius: i32,
    pub fog_radius: i32,
    pub public_players: Vec<PublicPlayer>,
    /// Row-major over the (2R+1)^2 axial bounding grid; `None` for off-board or (if
    /// `public_terrain` is off) unseen cells.
    pub public_terrain: Vec<Option<Terrain>>,
    /// Visible hexes, sorted by (r, q). Hidden mutable state is omitted entirely.
    pub visible_hexes: Vec<VisibleHex>,
    pub own_economy: PlayerEconomy,
    pub own_queue: Vec<Action>,
    pub visible_events: Vec<Event>,
    /// Hexes farther than this from the centre are in the storm (see
    /// [`RulesConfig::storm_radius`]); `None` while the whole board is safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storm_radius: Option<i32>,
    /// The match's rules, sent with the tick-0 view only. Bots should play by these
    /// rather than the defaults they were compiled with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<RulesConfig>,
}

pub type FixedPoints = FixedScore;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub player: PlayerId,
    pub place_start: u16,
    pub place_end: u16,
    /// Milli-points: average of N - r + 1 over the occupied places.
    pub points: FixedScore,
    pub eliminated_at: Option<Tick>,
    pub land: u32,
    pub troops: u64,
    pub resource: u32,
    pub buildings: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchResult {
    pub rules_version: String,
    pub final_tick: Tick,
    /// Best finish first.
    pub placements: Vec<Placement>,
}

impl MatchResult {
    pub fn placement(&self, p: PlayerId) -> &Placement {
        self.placements
            .iter()
            .find(|x| x.player == p)
            .expect("player in result")
    }
    /// Set only when a single player finished strictly first.
    pub fn winner(&self) -> Option<PlayerId> {
        self.placements
            .first()
            .filter(|f| f.place_start == 1 && f.place_end == 1)
            .map(|f| f.player)
    }
    /// Average place (e.g. 3.5 for a tie across 3rd and 4th).
    pub fn place_f64(&self, p: PlayerId) -> f64 {
        let x = self.placement(p);
        (x.place_start as f64 + x.place_end as f64) / 2.0
    }
    pub fn points_f64(&self, p: PlayerId) -> f64 {
        self.placement(p).points as f64 / MILLI as f64
    }
    /// (N - place) / (N - 1)
    pub fn normalized(&self, p: PlayerId) -> f64 {
        let n = self.placements.len() as f64;
        if n < 2.0 {
            1.0
        } else {
            (n - self.place_f64(p)) / (n - 1.0)
        }
    }
    pub fn survival_ticks(&self, p: PlayerId) -> Tick {
        self.placement(p).eliminated_at.unwrap_or(self.final_tick)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPlayer {
    pub id: PlayerId,
    pub name: String,
}

pub type TickCommands = Vec<(PlayerId, PlayerCommand)>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replay {
    pub protocol_version: u32,
    pub engine_version: String,
    pub rules_version: String,
    pub rules_hash: String,
    pub rules: RulesConfig,
    pub terrain_seed: u64,
    pub spawn_seed: u64,
    pub match_seed: u64,
    pub players: Vec<ReplayPlayer>,
    pub commands: Vec<TickCommands>,
    /// Full authoritative events per tick (unfiltered), for debugging.
    pub events: Vec<Vec<Event>>,
    pub result: MatchResult,
    pub checksum: String,
}

impl Replay {
    pub fn settings(&self) -> MatchSettings {
        MatchSettings {
            players: self.players.len() as u16,
            terrain_seed: self.terrain_seed,
            spawn_seed: self.spawn_seed,
            match_seed: self.match_seed,
            rules: self.rules.clone(),
        }
    }
}
