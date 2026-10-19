export type Hex = { q: number; r: number }
export type Terrain = { height: number; kind: 'open' | 'mountain' | 'water' } | null

export type GameEvent =
  | { type: 'moved'; player: number; from: Hex; to: Hex; troops: number }
  | { type: 'gathered'; player: number; target: Hex; sources: number }
  | { type: 'captured'; player: number; at: Hex; previous_owner: number | null; troops_left: number; building: string | null }
  | { type: 'repelled'; attacker: number; at: Hex; attackers_lost: number }
  | { type: 'surrendered'; capturer: number; from: number; hexes: Hex[] }
  | { type: 'built'; player: number; at: Hex; building: string }
  | { type: 'demolished'; player: number; at: Hex; building: string }
  | { type: 'link_changed'; player: number; at: Hex }
  | { type: 'links_broken'; at: Hex }
  | { type: 'eliminated'; player: number }
  | { type: 'rejected'; player: number; reason: string }

export interface Rules {
  version: string
  max_ticks: number
  /** Absent in v0 replays, which ended when one player was left. */
  end_on_total_conquest?: boolean
  fog_radius: number
  starting_troops: number
  base_income: number
  extractor_period: number
  /** 0 from v0.4: Farms and Quarries make no Resource and every build costs troops only. */
  extractor_output: number
  attacker_exchange_win_milli: number
  overtime_start?: number
  overtime_win_milli_per_1000?: number
  overtime_max_win_milli?: number
  storm_start?: number
  storm_end?: number
  storm_decay_pct?: number
  height_factor_milli: number
  maximum_total_defense_milli: number
  /** `adjacent_boost` (v0.5+): every adjacent Farm boosts a Village, no links or sockets. */
  farm: { troop_cost: number; resource_cost: number; allowed_heights: number[]; sockets: number; relink_cooldown: number; adjacent_boost?: boolean }
  village: { troop_cost: number; resource_cost: number; bonus: number; farm_bonus: number }
  quarry: { troop_cost: number; resource_cost: number; allowed_heights: number[]; min_friendly_distance: number; discount_radius: number; troop_discount_pct: number; resource_discount: number; village_bonus?: number }
  wall: { troop_cost: number; resource_cost: number; max_links: number; defense_milli_by_links: number[]; relink_cooldown: number }
  /** v0.7+: Go-style capture of any region sealed off by one player's land. */
  region_capture?: boolean
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
  /** Raw frames as served; read them through `store`. */
  frames: number[][]
  store: FrameStore
  events: GameEvent[][]
  result: { rules_version: string; final_tick: number; placements: Placement[] }
  building_report: { matches: number; farm: KindReport; village: KindReport; quarry: KindReport; wall: KindReport }
}

export const BUILDING_KEYS = ['farm', 'village', 'quarry', 'wall'] as const
export const BUILDING_NAMES = ['Farm', 'Village', 'Quarry', 'Wall']

export const PLAYER_COLORS = ['#3b82f6', '#ef4444', '#facc15', '#22d3ee', '#e879f9', '#fb923c', '#a3e635', '#f4f4f5']
export const PLAYER_MARKS = ['●', '■', '▲', '◆', '★', '✚', '⬟', '▼']

/** Distinct height ramp: deep blue valleys, green plains, tan to rust hills. */
export const HEIGHT_STYLE: Record<number, { fill: string; tag: string; label: string; name: string }> = {
  [-2]: { fill: '#2a2446', tag: '#b9aef0', label: '−2', name: 'Deep valley' },
  [-1]: { fill: '#3e3663', tag: '#cfc6fb', label: '−1', name: 'Valley' },
  [0]: { fill: '#2f4a2e', tag: '#b7d3a8', label: '0', name: 'Plain' },
  [1]: { fill: '#575b63', tag: '#e4e6ea', label: '+1', name: 'Hill' },
  [2]: { fill: '#878a92', tag: '#f6f7f9', label: '+2', name: 'High hill' },
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
export const usesResource = (r: Rules) => r.extractor_output > 0

/** Distance from the centre of the innermost passable hex: the storm stops there. */
export function stormFloor(rep: ViewerReplay): number {
  let best = rep.radius
  rep.terrain.forEach((t, i) => {
    if (t && t.kind === 'open') best = Math.min(best, hexDist(hexAt(rep.radius, i), { q: 0, r: 0 }))
  })
  return best
}

/** Mirrors `RulesConfig::storm_radius`: safe distance from the centre, or null while the board is safe. */
export function stormRadius(r: Rules, tick: number, radius: number, floor: number): number | null {
  const start = r.storm_start ?? 0, end = r.storm_end ?? 0
  if (end <= start || tick <= start) return null
  const left = Math.max(0, end - tick)
  const v = floor + Math.ceil((Math.max(0, radius - floor) * left) / (end - start))
  return v < radius ? v : null
}

export const hexIndex = (radius: number, h: Hex) => (h.r + radius) * (2 * radius + 1) + h.q + radius
export const onBoard = (radius: number, h: Hex) =>
  Math.abs(h.q) <= radius && Math.abs(h.r) <= radius && Math.abs(h.q + h.r) <= radius
export const addHex = (a: Hex, b: Hex): Hex => ({ q: a.q + b.q, r: a.r + b.r })

/** Same-owner Quarries next to the Village at `idx` that boost it (`quarry.village_bonus`, v0.6+). */
export function adjacentQuarries(rep: ViewerReplay, cells: Map<number, Cell>, idx: number): number {
  const c = cells.get(idx)
  if (!rep.rules.quarry.village_bonus || !c || c.owner < 0 || c.building !== 2) return 0
  const h = hexAt(rep.radius, idx)
  return HEX_DIRS.filter((d) => {
    const n = addHex(h, d)
    const x = onBoard(rep.radius, n) ? cells.get(hexIndex(rep.radius, n)) : undefined
    return x?.owner === c.owner && x.building === 3
  }).length
}

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

const DELTA_REMOVED = 1 << 8
/** Record fields (owner, troops, building, remaining, link×3, cooldown) the encoder predicts linearly. */
const DELTA_PREDICTED = [1, 3, 7]
const FIELDS = 8
const KEYFRAME_EVERY = 64

interface Snapshot { s: Int32Array; present: Uint8Array; vel: Int32Array }

/**
 * Random access to the frames of a `delta-v2` viewer document (see `frame_delta` in the
 * arena crate) without expanding them all: one decoding pass stores a snapshot every
 * 64 ticks, and any tick is rebuilt from the nearest snapshot or the last tick shown.
 */
export class FrameStore {
  readonly last: number
  private s: Int32Array
  private present: Uint8Array
  private vel: Int32Array
  private old: Int32Array
  private was: Uint8Array
  private at = 0
  private keys: Snapshot[] = []
  private raw: number[][]
  private size: number
  private full: boolean

  constructor(raw: number[][], size: number, full: boolean) {
    this.raw = raw; this.size = size; this.full = full
    this.last = raw.length - 1
    this.s = new Int32Array(size * FIELDS)
    this.present = new Uint8Array(size)
    this.vel = new Int32Array(size * 3)
    this.old = new Int32Array(size * 3)
    this.was = new Uint8Array(size)
    if (full) return
    this.load(raw[0])
    for (let t = 0; t <= this.last; t++) {
      if (t > 0) this.step(raw[t])
      if (t % KEYFRAME_EVERY === 0) this.keys.push({ s: this.s.slice(), present: this.present.slice(), vel: this.vel.slice() })
    }
    this.restore(0)
  }

  private load(f: number[]) {
    this.s.fill(0); this.present.fill(0); this.vel.fill(0)
    for (let i = 0; i < f.length; i += 9) {
      const idx = f[i]
      this.present[idx] = 1
      for (let k = 0; k < FIELDS; k++) this.s[idx * FIELDS + k] = f[i + 1 + k]
    }
  }

  private restore(t: number) {
    const k = this.keys[Math.floor(t / KEYFRAME_EVERY)]
    this.s.set(k.s); this.present.set(k.present); this.vel.set(k.vel)
    this.at = Math.floor(t / KEYFRAME_EVERY) * KEYFRAME_EVERY
  }

  private step(d: number[]) {
    const { s, present, vel, old, was, size } = this
    for (let idx = 0; idx < size; idx++) {
      if (!present[idx]) continue
      was[idx] = 1
      for (let j = 0; j < 3; j++) {
        const at = idx * FIELDS + DELTA_PREDICTED[j]
        old[idx * 3 + j] = s[at]
        s[at] += vel[idx * 3 + j]
      }
    }
    let i = 0
    while (i < d.length) {
      const idx = d[i], mask = d[i + 1]
      i += 2
      if (mask & DELTA_REMOVED) { present[idx] = 0; continue }
      if (!present[idx]) { s.fill(0, idx * FIELDS, idx * FIELDS + FIELDS); present[idx] = 1 }
      for (let k = 0; k < FIELDS; k++) if (mask & (1 << k)) s[idx * FIELDS + k] = d[i++]
    }
    for (let idx = 0; idx < size; idx++) {
      for (let j = 0; j < 3; j++)
        vel[idx * 3 + j] = present[idx] && was[idx] ? s[idx * FIELDS + DELTA_PREDICTED[j]] - old[idx * 3 + j] : 0
      was[idx] = 0
    }
  }

  private seek(t: number) {
    if (this.full) { this.load(this.raw[t]); return }
    if (t < this.at || t - this.at > KEYFRAME_EVERY) this.restore(t)
    while (this.at < t) this.step(this.raw[++this.at])
  }

  cells(tick: number): Map<number, Cell> {
    this.seek(Math.max(0, Math.min(this.last, tick)))
    const m = new Map<number, Cell>()
    const s = this.s
    for (let idx = 0; idx < this.size; idx++) {
      if (!this.present[idx]) continue
      const o = idx * FIELDS
      m.set(idx, { owner: s[o], troops: s[o + 1], building: s[o + 2], remaining: s[o + 3], links: [s[o + 4], s[o + 5], s[o + 6]], cooldown: s[o + 7] })
    }
    return m
  }
}

/** Wraps a fetched viewer document with random access to its frames. */
export function loadViewer(raw: Omit<ViewerReplay, 'store'> & { frame_encoding?: string }): ViewerReplay {
  const size = (2 * raw.radius + 1) ** 2
  return { ...raw, store: new FrameStore(raw.frames, size, raw.frame_encoding !== 'delta-v2') }
}

export function frameCells(rep: ViewerReplay, tick: number): Map<number, Cell> {
  return rep.store.cells(tick)
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
  else if (kind === 1 && !r.farm.allowed_heights.includes(t.height)) reason = 'no Farms on hills'
  else if (kind === 3 && !r.quarry.allowed_heights.includes(t.height)) reason = 'Quarries need height +1 or +2'
  else if (kind === 3 && near(r.quarry.min_friendly_distance - 1, 3, owner)) reason = `within ${r.quarry.min_friendly_distance - 1} hexes of your Quarry`
  return { kind, legal: reason === null, reason, baseTroops: rule.troop_cost, baseResource: rule.resource_cost, troops, resource, discounted: disc }
}
