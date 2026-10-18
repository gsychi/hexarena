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

export interface MatchSummary {
  id: string
  created_at: number
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
  avg_normalized: number
  history: number[]
}
export interface Leaderboard { games: number; start_elo: number; k: number; bots: LeaderboardRow[] }

export interface CreateMatch { bots: string[]; seed?: number; max_ticks: number; count: number }

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
  create: (req: CreateMatch) =>
    fetch('/api/matches', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(req) }).then((r) => json<MatchSummary[]>(r)),
}
