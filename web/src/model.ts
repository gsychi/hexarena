export type Hex = { q: number; r: number }
export type Terrain = { height: number; kind: 'open' | 'mountain' | 'water' } | null

export type GameEvent =
  | { type: 'moved'; player: number; from: Hex; to: Hex; troops: number }
  | { type: 'gathered'; player: number; target: Hex; sources: number }
  | { type: 'captured'; player: number; at: Hex; previous_owner: number | null; troops_left: number; building: string | null }
  | { type: 'repelled'; attacker: number; at: Hex; attackers_lost: number }
  | { type: 'surrendered'; capturer: number; from: number; hexes: Hex[] }
  | { type: 'built'; player: number; at: Hex; building: string }
  | { type: 'link_changed'; player: number; at: Hex }
  | { type: 'links_broken'; at: Hex }
  | { type: 'eliminated'; player: number }
  | { type: 'rejected'; player: number; reason: string }

export interface Rules {
  version: string
  fog_radius: number
  starting_troops: number
  base_income: number
  extractor_period: number
  attacker_exchange_win_milli: number
  height_factor_milli: number
  maximum_total_defense_milli: number
  farm: { troop_cost: number; resource_cost: number; allowed_heights: number[]; sockets: number; relink_cooldown: number }
  village: { troop_cost: number; resource_cost: number; bonus: number; farm_bonus: number }
  quarry: { troop_cost: number; resource_cost: number; allowed_heights: number[]; min_friendly_distance: number; discount_radius: number; troop_discount_pct: number; resource_discount: number }
  wall: { troop_cost: number; resource_cost: number; max_links: number; defense_milli_by_links: number[]; relink_cooldown: number }
}

export interface KindReport {
  constructed: number
  avg_construction_tick: number
  avg_troops_paid: number
  captured_share: number
  avg_resource_generated: number
  avg_village_bonus_troops: number
  linked_share: number
  avg_wall_multiplier: number
  avg_wall_fights: number
  repaid_share: number
  avg_builder_place: number
}

export interface Placement { player: number; place_start: number; place_end: number; points: number; land: number }

export interface ViewerReplay {
  radius: number
  fog: number
  names: string[]
  rules: Rules
  terrain: Terrain[]
  neutral: number[]
  frames: number[][]
  events: GameEvent[][]
  result: { rules_version: string; final_tick: number; placements: Placement[] }
  building_report: { matches: number; farm: KindReport; village: KindReport; quarry: KindReport; wall: KindReport }
}

export const BUILDING_KEYS = ['farm', 'village', 'quarry', 'wall'] as const
export const BUILDING_NAMES = ['Farm', 'Village', 'Quarry', 'Wall']

export const PLAYER_COLORS = ['#3b82f6', '#ef4444', '#22c55e', '#f59e0b', '#a855f7', '#06b6d4', '#ec4899', '#e5e7eb']
export const PLAYER_MARKS = ['●', '■', '▲', '◆', '★', '✚', '⬟', '▼']

/** Distinct height ramp: deep blue valleys, green plains, tan to rust hills. */
export const HEIGHT_STYLE: Record<number, { fill: string; tag: string; label: string; name: string }> = {
  [-2]: { fill: '#3a2a68', tag: '#c9b8ff', label: '−2', name: 'deep valley' },
  [-1]: { fill: '#5a4a8c', tag: '#ddd0ff', label: '−1', name: 'valley' },
  [0]: { fill: '#3e5c52', tag: '#c8e6da', label: '0', name: 'plain' },
  [1]: { fill: '#7b818d', tag: '#f1f3f7', label: '+1', name: 'hill' },
  [2]: { fill: '#aeb4c0', tag: '#20232b', label: '+2', name: 'high hill' },
}

export const HEX_DIRS: Hex[] = [
  { q: 1, r: 0 }, { q: 1, r: -1 }, { q: 0, r: -1 }, { q: -1, r: 0 }, { q: -1, r: 1 }, { q: 0, r: 1 },
]

export const hexDist = (a: Hex, b: Hex) =>
  (Math.abs(a.q - b.q) + Math.abs(a.r - b.r) + Math.abs(a.q + a.r - b.q - b.r)) / 2

export function hexAt(radius: number, idx: number): Hex {
  const w = 2 * radius + 1
  return { q: (idx % w) - radius, r: Math.floor(idx / w) - radius }
}
export const hexIndex = (radius: number, h: Hex) => (h.r + radius) * (2 * radius + 1) + h.q + radius
export const onBoard = (radius: number, h: Hex) =>
  Math.abs(h.q) <= radius && Math.abs(h.r) <= radius && Math.abs(h.q + h.r) <= radius
export const addHex = (a: Hex, b: Hex): Hex => ({ q: a.q + b.q, r: a.r + b.r })

export interface Cell {
  owner: number
  troops: number
  /** 0 none, 1 farm, 2 village, 3 quarry, 4 wall */
  building: number
  remaining: number
  /** Farm: village per socket. Village: [supporting farm]. Wall: partners. Hex indices, -1 empty. */
  links: [number, number, number]
  cooldown: number
}

export function frameCells(rep: ViewerReplay, tick: number): Map<number, Cell> {
  const f = rep.frames[tick]
  const m = new Map<number, Cell>()
  for (let i = 0; i < f.length; i += 9)
    m.set(f[i], {
      owner: f[i + 1], troops: f[i + 2], building: f[i + 3], remaining: f[i + 4],
      links: [f[i + 5], f[i + 6], f[i + 7]], cooldown: f[i + 8],
    })
  return m
}

/** Display-side fog mask for rendering; mirrors the engine's vision rule. */
export function visibleSet(rep: ViewerReplay, cells: Map<number, Cell>, player: number): Set<number> {
  const out = new Set<number>()
  const v = rep.fog
  for (const [idx, c] of cells) {
    if (c.owner !== player) continue
    const h = hexAt(rep.radius, idx)
    for (let dq = -v; dq <= v; dq++)
      for (let dr = Math.max(-v, -dq - v); dr <= Math.min(v, -dq + v); dr++) {
        const n = { q: h.q + dq, r: h.r + dr }
        if (onBoard(rep.radius, n)) out.add(hexIndex(rep.radius, n))
      }
  }
  return out
}

const roundDiv = (a: number, b: number) => Math.floor((a * 1000 + b / 2) / b)

/** Display-only mirror of the engine's height multiplier (milli). */
export function heightMultMilli(rules: Rules, attacker: number, defender: number): number {
  const f = rules.height_factor_milli
  const d = defender - attacker
  let m = 1000
  for (let i = 0; i < Math.abs(d); i++) m = d > 0 ? Math.floor((m * f + 500) / 1000) : roundDiv(m, f)
  return m
}

export function defenseMilli(rules: Rules, attacker: number, defender: number, wallLinks: number | null): number {
  const h = heightMultMilli(rules, attacker, defender)
  const w = wallLinks === null ? 1000 : rules.wall.defense_milli_by_links[Math.min(wallLinks, rules.wall.defense_milli_by_links.length - 1)]
  return Math.min(Math.floor((h * w) / 1000), rules.maximum_total_defense_milli)
}

export interface BuildPreview {
  kind: number
  legal: boolean
  reason: string | null
  baseTroops: number
  baseResource: number
  troops: number
  resource: number
  discounted: boolean
}

/** Display-only preview of what building here would cost; the engine remains authoritative. */
export function buildPreview(rep: ViewerReplay, cells: Map<number, Cell>, idx: number, kind: number): BuildPreview {
  const r = rep.rules
  const cell = cells.get(idx)
  const t = rep.terrain[idx]
  const h = hexAt(rep.radius, idx)
  const key = BUILDING_KEYS[kind - 1]
  const rule = r[key]
  const near = (radius: number, k: number, owner: number) => {
    for (const [i, c] of cells)
      if (i !== idx && c.owner === owner && c.building === k && hexDist(hexAt(rep.radius, i), h) <= radius) return true
    return false
  }
  const owner = cell?.owner ?? -1
  const disc = owner >= 0 && kind !== 3 && near(r.quarry.discount_radius, 3, owner)
  const troops = disc ? Math.ceil((rule.troop_cost * (100 - r.quarry.troop_discount_pct)) / 100) : rule.troop_cost
  const resource = disc ? Math.max(0, rule.resource_cost - r.quarry.resource_discount) : rule.resource_cost
  let reason: string | null = null
  if (!t || t.kind !== 'open') reason = 'impassable'
  else if (owner < 0) reason = 'not owned'
  else if (cell && cell.building > 0) reason = 'hex already has a building'
  else if (kind === 1 && !r.farm.allowed_heights.includes(t.height)) reason = 'Farms need height −2 or −1'
  else if (kind === 3 && !r.quarry.allowed_heights.includes(t.height)) reason = 'Quarries need height +1 or +2'
  else if (kind === 3 && near(r.quarry.min_friendly_distance - 1, 3, owner)) reason = `within ${r.quarry.min_friendly_distance - 1} hexes of your Quarry`
  return { kind, legal: reason === null, reason, baseTroops: rule.troop_cost, baseResource: rule.resource_cost, troops, resource, discounted: disc }
}
