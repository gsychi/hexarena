export type Hex = { q: number; r: number }
export type Terrain = { height: number; kind: 'open' | 'mountain' | 'water' } | null
export type GameEvent =
  | { type: 'moved'; player: number; from: Hex; to: Hex; troops: number }
  | { type: 'captured'; player: number; at: Hex; previous_owner: number | null; troops_left: number }
  | { type: 'repelled'; attacker: number; at: Hex; attackers_lost: number }
  | { type: 'built'; player: number; at: Hex; building: string }
  | { type: 'eliminated'; player: number }
  | { type: 'rejected'; player: number; reason: string }

export interface ViewerReplay {
  radius: number
  vision: number
  names: string[]
  terrain: Terrain[]
  neutral: number[]
  frames: number[][]
  events: GameEvent[][]
  result: { places: number[]; points: number[]; placement_order: number[][]; ticks: number }
  rules_version: string
}

export const BUILDINGS = ['F', 'V', 'Q', 'W'] as const
export const BUILDING_NAMES = ['Farm', 'Village', 'Quarry', 'Wall']

export const PLAYER_COLORS = ['#3b82f6', '#ef4444', '#22c55e', '#f59e0b', '#a855f7', '#06b6d4', '#ec4899', '#e5e7eb']
export const PLAYER_MARKS = ['●', '■', '▲', '◆', '★', '✚', '⬟', '▼']

export const hexDist = (a: Hex, b: Hex) =>
  (Math.abs(a.q - b.q) + Math.abs(a.r - b.r) + Math.abs(a.q + a.r - b.q - b.r)) / 2

export function hexAt(radius: number, idx: number): Hex {
  const w = 2 * radius + 1
  return { q: (idx % w) - radius, r: Math.floor(idx / w) - radius }
}
export const hexIndex = (radius: number, h: Hex) => (h.r + radius) * (2 * radius + 1) + h.q + radius
export const onBoard = (radius: number, h: Hex) =>
  Math.abs(h.q) <= radius && Math.abs(h.r) <= radius && Math.abs(h.q + h.r) <= radius

export interface Cell { owner: number; troops: number; building: number }

export function frameCells(rep: ViewerReplay, tick: number): Map<number, Cell> {
  const f = rep.frames[tick]
  const m = new Map<number, Cell>()
  for (let i = 0; i < f.length; i += 4) m.set(f[i], { owner: f[i + 1], troops: f[i + 2], building: f[i + 3] })
  return m
}

/** Display-side fog mask for a player; mirrors the server's vision rule for rendering only. */
export function visibleSet(rep: ViewerReplay, cells: Map<number, Cell>, player: number): Set<number> {
  const out = new Set<number>()
  const v = rep.vision
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
