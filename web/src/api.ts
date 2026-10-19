export interface BotInfo { id: string; name: string; blurb: string }

export interface Seat { bot: string; name: string }

export interface Standing {
  player: number
  name: string
  bot: string
  place_start: number
  place_end: number
  points: number
  normalized: number
  land: number
}

export type MatchStatus = 'running' | 'done' | 'failed'
export type Ending = 'conquest' | 'last_standing' | 'cutoff'

export interface MatchSummary {
  id: string
  created_at: number
  rules_version: string
  ending: Ending | null
  status: MatchStatus
  error: string | null
  seed: number
  max_ticks: number
  seats: Seat[]
  final_tick: number | null
  winner: string | null
  standings: Standing[]
}

export interface LeaderboardRow {
  rank: number
  bot: string
  name: string
  elo: number
  elo_change: number
  peak_elo: number
  games: number
  seats: number
  wins: number
  win_rate: number
  top_half: number
  top_half_rate: number
  avg_place: number
  avp8: number
  avg_normalized: number
  history: number[]
}
export interface Leaderboard { games: number; rules_version: string; start_elo: number; k: number; bots: LeaderboardRow[] }

export interface CreateMatch { bots: string[]; seed?: number; count: number }

export interface GateResult {
  games: number
  challenger_avp: number
  champion_avp: number
  gain: number
  gain_se: number
  promoted: boolean
  secs: number
}

export interface Benchmark { games: number; avp: number; win_rate: number; top_half_rate: number; close_rate?: number; avg_close_tick?: number | null }

export interface Generation {
  gen: number
  id: string
  /** Brain plus lineage, e.g. "Relay g3 ← g2" or "Relay g2 (new brain by agent tempo)". */
  name?: string
  /** Generations it was derived from; empty for a seed or a brand-new brain. */
  parents?: string[]
  /** The champion it beat to be promoted. */
  beat?: string | null
  origin: string
  note: string | null
  created_at: number
  params: Record<string, unknown>
  changes: string[]
  gate: GateResult | null
  benchmark: Benchmark
  elo: number
}

export interface Attempt {
  n: number
  at: number
  id: string
  origin: string
  note: string | null
  parent: string
  changes: string[]
  gate: GateResult
  became: number | null
}

export interface League {
  rules_version: string
  started_at: number
  updated_at: number
  gate_config: { min_pairs: number; max_pairs: number; batch_pairs: number; min_gain: number; z_accept: number; close_bonus?: number; close_speed_bonus?: number }
  generations: Generation[]
  attempts: Attempt[]
  attempts_total: number
  games_total: number
  play_secs: number
  elo: Record<string, number>
}

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    throw new Error(body?.error ?? `${res.status} ${res.statusText}`)
  }
  return res.json()
}

export const api = {
  bots: () => fetch('/api/bots').then((r) => json<BotInfo[]>(r)),
  matches: () => fetch('/api/matches').then((r) => json<MatchSummary[]>(r)),
  match: (id: string) => fetch(`/api/matches/${id}`).then((r) => json<MatchSummary>(r)),
  leaderboard: () => fetch('/api/leaderboard').then((r) => json<Leaderboard>(r)),
  league: () => fetch('/api/league').then((r) => json<League>(r)),
  create: (req: CreateMatch) =>
    fetch('/api/matches', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(req) }).then((r) => json<MatchSummary[]>(r)),
}
