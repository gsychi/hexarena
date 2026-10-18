//! Per-building instrumentation and aggregate reports for balance analysis.

use hexarena_protocol::{Building, Hex, MatchResult, PlayerId};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct BuildRecord {
    pub kind: Building,
    pub builder: PlayerId,
    pub tick: u32,
    pub hex: Hex,
    pub height: i8,
    pub troops_paid: u32,
    pub resource_paid: u32,
    pub troop_discount: u32,
    pub resource_discount: u32,
    /// Ticks owned, indexed by player.
    pub owned_ticks: Vec<u32>,
    pub captures: Vec<(u32, PlayerId)>,
    pub resource_generated: u32,
    pub village_bonus_troops: u32,
    pub linked_ticks: u32,
    pub unlinked_ticks: u32,
    pub wall_mult_x100_sum: u64,
    pub wall_mult_ticks: u32,
    pub wall_fights: u32,
    /// Villages only: tick at which bonus troops produced first covered the troop cost.
    pub repaid_tick: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct KindReport {
    pub constructed: u32,
    pub avg_construction_tick: f64,
    pub avg_troops_paid: f64,
    pub avg_resource_paid: f64,
    pub quarry_discounted_share: f64,
    pub avg_troop_discount: f64,
    pub captured_share: f64,
    pub avg_resource_generated: f64,
    pub avg_village_bonus_troops: f64,
    /// Share of owned time with at least one active link / support.
    pub linked_share: f64,
    pub avg_wall_multiplier: f64,
    pub avg_wall_fights: f64,
    /// Share of Villages that survived long enough to repay their troop cost.
    pub repaid_share: f64,
    pub avg_builder_place: f64,
    pub avg_captor_place: f64,
    pub height_histogram: [u32; 5],
    #[serde(skip)]
    sums: Sums,
}

#[derive(Clone, Debug, Default)]
struct Sums {
    tick: f64,
    troops: f64,
    resource: f64,
    discounted: f64,
    troop_discount: f64,
    generated: f64,
    bonus: f64,
    linked: f64,
    unlinked: f64,
    mult: f64,
    mult_ticks: f64,
    fights: f64,
    repaid: f64,
    builder_place: f64,
    captor_place: f64,
    captured_n: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct BuildingReport {
    pub matches: u32,
    pub farm: KindReport,
    pub village: KindReport,
    pub quarry: KindReport,
    pub wall: KindReport,
}

impl BuildingReport {
    pub fn kind_mut(&mut self, k: Building) -> &mut KindReport {
        match k {
            Building::Farm => &mut self.farm,
            Building::Village => &mut self.village,
            Building::Quarry => &mut self.quarry,
            Building::Wall => &mut self.wall,
        }
    }

    /// Accumulate one finished match. Call `finalize` once after all matches.
    pub fn add_match(&mut self, records: &[BuildRecord], result: &MatchResult) {
        self.matches += 1;
        for r in records {
            let k = self.kind_mut(r.kind);
            k.constructed += 1;
            let s = &mut k.sums;
            s.tick += r.tick as f64;
            s.troops += r.troops_paid as f64;
            s.resource += r.resource_paid as f64;
            s.discounted += (r.troop_discount > 0 || r.resource_discount > 0) as u32 as f64;
            s.troop_discount += r.troop_discount as f64;
            s.generated += r.resource_generated as f64;
            s.bonus += r.village_bonus_troops as f64;
            s.linked += r.linked_ticks as f64;
            s.unlinked += r.unlinked_ticks as f64;
            s.mult += r.wall_mult_x100_sum as f64;
            s.mult_ticks += r.wall_mult_ticks as f64;
            s.fights += r.wall_fights as f64;
            s.repaid += r.repaid_tick.is_some() as u32 as f64;
            s.builder_place += result.place_f64(r.builder);
            if let Some((_, c)) = r.captures.last() {
                s.captured_n += 1.0;
                s.captor_place += result.place_f64(*c);
            }
            k.height_histogram[(r.height + 2) as usize] += 1;
        }
    }

    pub fn finalize(&mut self) {
        for b in [Building::Farm, Building::Village, Building::Quarry, Building::Wall] {
            let k = self.kind_mut(b);
            let n = k.constructed.max(1) as f64;
            let s = k.sums.clone();
            k.avg_construction_tick = s.tick / n;
            k.avg_troops_paid = s.troops / n;
            k.avg_resource_paid = s.resource / n;
            k.quarry_discounted_share = s.discounted / n;
            k.avg_troop_discount = s.troop_discount / n;
            k.captured_share = s.captured_n / n;
            k.avg_resource_generated = s.generated / n;
            k.avg_village_bonus_troops = s.bonus / n;
            k.linked_share = if s.linked + s.unlinked > 0.0 { s.linked / (s.linked + s.unlinked) } else { 0.0 };
            k.avg_wall_multiplier = if s.mult_ticks > 0.0 { s.mult / s.mult_ticks / 100.0 } else { 0.0 };
            k.avg_wall_fights = s.fights / n;
            k.repaid_share = s.repaid / n;
            k.avg_builder_place = s.builder_place / n;
            k.avg_captor_place = if s.captured_n > 0.0 { s.captor_place / s.captured_n } else { 0.0 };
        }
    }
}
