import { registerBots } from './lineage'
import type { Rules } from './model'

export interface BotInfo {
  id: string
  name: string
  blurb: string
  kind?: 'roster' | 'entrant'
  /** Arena entrants only: who published the version. */
  author?: string
  /** The bots it takes its policy from, as ids; empty for a design from scratch. */
  parents?: string[]
  own_build?: boolean
  /** Why it was retired from the arena; retired bots stay playable. */
  retired?: string | null
}

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
  /** Scheduled by the self-playing arena. */
  auto?: boolean
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
  /** Standard error of avp8. */
  avp8_se: number
  /** Played enough games (rated_games) for the rating to be trusted. */
  rated: boolean
  avg_normalized: number
  history: number[]
  /** Average decision time per move (mean of per-game averages), when timed. */
  ms_per_move?: number | null
  /** Half-width of the 95% confidence interval of ms_per_move. */
  ms_per_move_ci95?: number | null
  /** Slowest single move in any timed game. */
  slowest_ms?: number | null
  timed_games?: number
  /** On the overall board: the bot's rating from 1v1 games only, null before its first. */
  duel?: FormatRating | null
  /** On the overall board: the bot's rating from free-for-alls (3+ players) only. */
  ffa?: FormatRating | null
}
export interface FormatRating { elo: number; games: number; rank: number }
/** Which games a board rates on: every game, 1v1s only, or free-for-alls (3+ players) only. */
export type BoardFormat = 'all' | 'duel' | 'ffa'
export interface Leaderboard { format?: BoardFormat; duel_games?: number; ffa_games?: number; games: number; rules_version: string; start_elo: number; top?: number; rated_games?: number; retired?: Record<string, string>; anchors?: string[]; anchor_elo?: number; bots: LeaderboardRow[]; retired_bots?: LeaderboardRow[] }

export interface ProfileGame {
  id: string
  created_at: number
  rules_version: string
  status: MatchStatus
  ending: Ending | null
  final_tick: number | null
  players: number
  winner: string | null
  auto: boolean
  place_start: number | null
  place_end: number | null
  ms_per_move: number | null
  opponents: string[]
}

export interface EloCurve { rules_version: string; games: number; elo: number; peak_elo: number; points: [number, number][] }

export interface BotProfile {
  bot: BotInfo & { brain?: string; created_at?: number }
  rules_version: string
  start_elo: number
  rated_games: number
  standing: { rank: number | null; row: LeaderboardRow } | null
  curves: EloCurve[]
  games: ProfileGame[]
  parents: Kin[]
  /** Versions that name this bot as a parent. */
  children: Kin[]
}

export interface Kin { id: string; name: string; retired: boolean }

/** Each bot's Elo going into a game and, once the game is rated, after it. */
export interface MatchElo { rules_version: string; rated: boolean; bots: { bot: string; name: string; before: number | null; after: number | null }[] }

export interface ArenaEntrant {
  id: string
  name: string
  author: string
  note: string | null
  created_at: number
  retired: boolean
  retired_reason: string | null
  games: number
  brain: string
  own_build: boolean
  parents: string[]
}

/** The self-playing arena: published bot versions and the retirement rule. */
export interface ArenaInfo { autoplay: boolean; top: number; trial_games: number; entrants: ArenaEntrant[]; retired: Record<string, string> }

/** The rules the server plays now, with the board and lobby settings replays leave out. */
export interface LiveRules extends Rules {
  min_players: number
  max_players: number
  map_radius_factor: number
  map_radius_exponent: number
  impassable_min_pct: number
  impassable_max_pct: number
  neutral_min: number
  neutral_max: number
  spawn_min_distance: number
  tick_rate_hz: number
}

export interface ChatMessage { n: number; at: number; agent: string; from: string; text: string }

/** Someone designing bots for the arena, as `GET /api/agents` reports them. */
export interface Agent {
  id: string
  model: string | null
  bots: number
  active_bots: number
  latest_bot: { id: string; name: string; at: number; retired: boolean } | null
  best_bot: { id: string; name: string; elo: number; rank: number } | null
  notes: number
  last_note: { n: number; at: number; kind: NoteKind; text: string } | null
  /** Last publish, note or reply, in ms; 0 if never. */
  last_seen: number
  messages: number
  last_message: ChatMessage | null
  /** Messages since the agent last replied. */
  unanswered: number
  /** What the agent says it is working on, if it has said. */
  status: AgentStatus | null
  /** The latest thing its tools reported, e.g. a local eval starting or finishing. */
  doing: AgentStatus | null
}

export interface AgentStatus { at: number; text: string }

export type ActivityItem =
  | { type: 'bot'; at: number; id: string; name: string; note: string | null; parents: string[]; retired: boolean; retired_reason: string | null; elo: number | null; rank: number | null; games: number }
  | { type: 'note'; at: number; n: number; kind: NoteKind; text: string; matches: string[]; bots: string[] }
  | { type: 'message'; at: number; n: number; from: string; text: string }
  | { type: 'work'; at: number; text: string }

export interface Activity { agent: string; status: AgentStatus | null; items: ActivityItem[] }

export interface CreateMatch { bots: string[]; seed?: number; count: number }

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    throw new Error(body?.error ?? `${res.status} ${res.statusText}`)
  }
  return res.json()
}

export const api = {
  /** Every bot, also recorded as lineage so mascots can inherit their parents' looks. */
  bots: () => fetch('/api/bots').then((r) => json<BotInfo[]>(r)).then((b) => { registerBots(b); return b }),
  profile: (id: string) => fetch(`/api/bots/${encodeURIComponent(id)}/profile`).then((r) => json<BotProfile>(r)),
  matches: () => fetch('/api/matches').then((r) => json<MatchSummary[]>(r)),
  /** The newest `limit` games plus how many games there are in all. */
  recentMatches: (limit: number) =>
    fetch(`/api/matches?limit=${limit}`).then(async (r) => ({ games: await json<MatchSummary[]>(r), total: Number(r.headers.get('x-total-count') ?? 0) })),
  match: (id: string) => fetch(`/api/matches/${id}`).then((r) => json<MatchSummary>(r)),
  rules: () => fetch('/api/rules').then((r) => json<LiveRules>(r)),
  leaderboard: (format: BoardFormat = 'all') => fetch(format === 'all' ? '/api/leaderboard' : `/api/leaderboard?format=${format}`).then((r) => json<Leaderboard>(r)),
  matchElo: (id: string) => fetch(`/api/matches/${id}/elo`).then((r) => json<MatchElo>(r)),
  arena: () => fetch('/api/arena').then((r) => json<ArenaInfo>(r)),
  agents: () => fetch('/api/agents').then((r) => json<Agent[]>(r)),
  activity: (agent: string) => fetch(`/api/agents/${encodeURIComponent(agent)}/activity`).then((r) => json<Activity>(r)),
  thread: (agent: string) => fetch(`/api/agents/${encodeURIComponent(agent)}/messages`).then((r) => json<ChatMessage[]>(r)),
  say: (agent: string, text: string) =>
    fetch(`/api/agents/${encodeURIComponent(agent)}/messages`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ from: 'you', text }) }).then((r) => json<ChatMessage>(r)),
  notes: () => fetch('/api/observations').then((r) => json<Note[]>(r)),
  addNote: (n: { author: string; kind: NoteKind; text: string; bots?: string[]; tags?: string[] }) =>
    fetch('/api/observations', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(n) }).then((r) => json<Note>(r)),
  create: (req: CreateMatch) =>
    fetch('/api/matches', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(req) }).then((r) => json<MatchSummary[]>(r)),
}

export type NoteKind = 'observation' | 'lesson' | 'idea'

/** One entry in the lab notebook: what someone saw in games and learned from it. */
export interface Note {
  n: number
  at: number
  author: string
  kind: NoteKind
  text: string
  bots: string[]
  matches: string[]
  tags: string[]
}

