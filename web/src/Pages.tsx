import { Dice5, Loader2, Play, Plus, Shuffle, Swords, TrendingUp, Trophy, Users } from 'lucide-react'
import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { BotInfo, MatchStatus, MatchSummary } from './api'
import { api } from './api'
import { PLAYER_COLORS } from './model'
import { AgentsPanel } from './Agents'
import { EloScatter } from './EloScatter'
import { Offline, Page } from './Page'
import { usePoll } from './poll'
import { Standings } from './Standings'

const BATCHES = [1, 5, 10, 20]
/** Games listed on the home page; the rest stay one click away in their replays. */
const RECENT = 40
const POLL_MS = 3000
const BOARD_MS = 10_000
const DEFAULT_LINEUP = ['expander', 'homesteader', 'raider', 'warden']

function finish(m: MatchSummary) {
  const at = `tick ${m.final_tick}`
  switch (m.ending) {
    case 'conquest':
      return `${m.winner} conquered the map at ${at}`
    case 'last_standing':
      return `${m.winner} last standing at ${at}`
    case 'cutoff':
      return `cut off at ${at}`
    case null:
      return `ended at ${at} (${m.rules_version} rules)`
    default: {
      const never: never = m.ending
      return never
    }
  }
}

function ago(ms: number) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return `${s}s ago`
  if (s < 3600) return `${Math.round(s / 60)}m ago`
  if (s < 86400) return `${Math.round(s / 3600)}h ago`
  return `${Math.round(s / 86400)}d ago`
}

function StatusBadge({ status }: { status: MatchStatus }) {
  switch (status) {
    case 'running': return <Badge variant="secondary" className="gap-1"><Loader2 className="size-3 animate-spin" />Running</Badge>
    case 'done': return <Badge variant="outline">Finished</Badge>
    case 'failed': return <Badge variant="destructive">Failed</Badge>
    default: { const _never: never = status; return _never }
  }
}

const loadBoard = () => api.leaderboard()
const loadProgress = () => Promise.all([api.leaderboard(), api.arena()])
const loadGames = () => Promise.all([api.recentMatches(RECENT), api.leaderboard()])

export function BotsPage({ onBot }: { onBot: (id: string) => void }) {
  const { data: board, error, refresh } = usePoll(loadBoard, BOARD_MS)
  return (
    <Page icon={Trophy} wide title="Bots" description="Who is sending bots in, and every bot in the arena ranked by Elo under the current rules. Click a bot to see its profile, family, rating curve and game log.">
      {error && <Offline error={error} onRetry={refresh} />}
      <AgentsPanel onBot={onBot} />
      <Standings board={board} onBot={onBot} />
    </Page>
  )
}

export function ProgressPage({ onBot }: { onBot: (id: string) => void }) {
  const { data, error, refresh } = usePoll(loadProgress, BOARD_MS)
  const [board, arena] = data ?? [null, null]
  return (
    <Page icon={TrendingUp} wide title="Progression"
      description={<>Every published version&apos;s current Elo{board ? ` under rules ${board.rules_version}` : ''} against when it went up. The green line is the frontier: the best Elo anything published so far has reached. Switch to ms per move for the speed–strength Pareto front, where nothing to the left of the line is stronger. Click a dot to open its profile.</>}>
      {error && <Offline error={error} onRetry={refresh} />}
      <Card>
        <CardContent>
          {board && arena
            ? <EloScatter rows={board.bots} retiredRows={board.retired_bots ?? []} entrants={arena.entrants} top={board.top ?? 20} onBot={onBot} />
            : !error && <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading ratings…</div>}
        </CardContent>
      </Card>
    </Page>
  )
}

export function GamesPage({ onOpen }: { onOpen: (id: string) => void }) {
  const [bots, setBots] = useState<BotInfo[]>([])
  const { data, error, refresh, setData } = usePoll(loadGames, POLL_MS)
  useEffect(() => { api.bots().then(setBots).catch(() => {}) }, [])
  const matches = data?.[0].games ?? null
  const total = data?.[0].total ?? 0
  const board = data?.[1] ?? null

  // Rounded ratings as a string, so the lineup picker only re-renders when a shown Elo changes.
  const eloKey = JSON.stringify([...(board?.bots ?? []), ...(board?.retired_bots ?? [])].map((r) => [r.bot, Math.round(r.elo)]))
  const elo = useMemo(() => new Map<string, number>(JSON.parse(eloKey)), [eloKey])
  const onCreated = useCallback((made: MatchSummary[]) => {
    setData((d) => (d ? [{ games: [...made, ...d[0].games].slice(0, RECENT), total: d[0].total + made.length }, d[1]] : d))
  }, [setData])

  return (
    <Page icon={Swords} wide title="Games" description="The self-playing arena keeps 8-player games going around the clock. Set up your own free-for-all, or open any game to watch its replay."
      actions={<Button variant="outline" size="sm" onClick={() => onOpen('sample')}><Play />Watch sample match</Button>}>
      {error && <Offline error={error} onRetry={refresh} />}
      <div className="grid gap-6 lg:grid-cols-[minmax(0,26rem)_minmax(0,1fr)]">
        <NewGame bots={bots} elo={elo} offline={!!error} onCreated={onCreated} />
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>Recent games</CardTitle>
            <CardDescription>{matches ? (total > matches.length ? `Newest ${matches.length} of ${total} games.` : `${total} game${total === 1 ? '' : 's'}, newest first.`) : 'Loading…'}</CardDescription>
          </CardHeader>
          <CardContent>
            {matches === null && !error && <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading games…</div>}
            {matches?.length === 0 && (
              <div className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
                No games yet. Pick a lineup and hit <span className="text-foreground">Create game</span>.
              </div>
            )}
            <ul className="divide-y">
              {matches?.map((m) => (
                <li key={m.id} className="flex flex-wrap items-center gap-x-4 gap-y-2 py-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-sm">
                      <span className="font-mono text-xs text-muted-foreground">{m.id}</span>
                      <StatusBadge status={m.status} />
                      {m.auto && <Badge variant="outline" className="text-[10px] text-muted-foreground">Self-play</Badge>}
                      {m.winner && <span className="flex min-w-0 items-center gap-1 truncate text-xs"><Trophy className="size-3 shrink-0 text-amber-300" />{m.winner}</span>}
                    </div>
                    <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
                      {m.seats.map((s, i) => (
                        <span key={i} className="flex items-center gap-1"><span className="size-2 rounded-full" style={{ background: PLAYER_COLORS[i % 8] }} />{s.name}</span>
                      ))}
                    </div>
                    <div className="mt-1 text-[11px] text-muted-foreground">
                      seed {m.seed} · {m.status === 'done' ? finish(m) : `up to ${m.max_ticks} ticks`} · {ago(m.created_at)}
                      {m.error && <span className="text-destructive"> · {m.error}</span>}
                    </div>
                  </div>
                  <Button size="sm" variant={m.status === 'done' ? 'default' : 'outline'} disabled={m.status !== 'done'} onClick={() => onOpen(m.id)}>
                    <Play />Watch
                  </Button>
                </li>
              ))}
            </ul>
          </CardContent>
        </Card>
      </div>
    </Page>
  )
}

const NewGame = memo(function NewGame({ bots, elo, offline, onCreated }: {
  bots: BotInfo[]
  elo: Map<string, number>
  offline: boolean
  onCreated: (made: MatchSummary[]) => void
}) {
  const [lineup, setLineup] = useState<string[]>(DEFAULT_LINEUP)
  const [seed, setSeed] = useState('')
  const [count, setCount] = useState(1)
  const [creating, setCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)

  const setPlayers = (n: number) => setLineup((l) => Array.from({ length: n }, (_, i) => l[i] ?? DEFAULT_LINEUP[i % DEFAULT_LINEUP.length]))
  const active = useMemo(() => bots.filter((b) => !b.retired), [bots])
  const randomize = () => setLineup((l) => l.map(() => active[Math.floor(Math.random() * active.length)]?.id ?? 'expander'))
  const items = useMemo(() => bots.map((b) => ({ value: b.id, label: b.name })), [bots])
  const groups = useMemo(() => {
    const byElo = (list: BotInfo[]) => [...list].sort((a, b) => (elo.get(b.id) ?? -Infinity) - (elo.get(a.id) ?? -Infinity) || a.name.localeCompare(b.name))
    return [
      ['Active', byElo(bots.filter((b) => !b.retired))],
      ['Retired', byElo(bots.filter((b) => b.retired))],
    ] as const
  }, [bots, elo])

  const create = async () => {
    setCreating(true); setCreateError(null)
    try {
      const s = seed.trim() === '' ? undefined : Number(seed)
      if (s !== undefined && (!Number.isSafeInteger(s) || s < 0)) throw new Error('Seed must be a whole number')
      onCreated(await api.create({ bots: lineup, seed: s, count }))
    } catch (e) {
      setCreateError(e instanceof Error ? e.message : String(e))
    } finally {
      setCreating(false)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>New game</CardTitle>
        <CardDescription>Every seat is a CPU. Same seed and lineup always give the same game.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="space-y-2">
          <Label className="flex items-center gap-1.5"><Users className="size-3.5" />Players</Label>
          <ToggleGroup variant="outline" size="sm" value={[String(lineup.length)]} onValueChange={(v) => v.length && setPlayers(Number(v[v.length - 1]))}>
            {[2, 3, 4, 5, 6, 7, 8].map((n) => <ToggleGroupItem key={n} value={String(n)} className="w-9 data-pressed:bg-foreground data-pressed:text-background">{n}</ToggleGroupItem>)}
          </ToggleGroup>
        </div>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <Label>Lineup</Label>
            <Button variant="ghost" size="xs" onClick={randomize} disabled={active.length === 0} title="Random bots that aren't retired"><Shuffle />Randomize</Button>
          </div>
          <div className="space-y-1.5">
            {lineup.map((id, i) => (
              <div key={i} className="flex items-center gap-2">
                <span className="size-2.5 shrink-0 rounded-full" style={{ background: PLAYER_COLORS[i % 8] }} />
                <span className="w-12 text-xs text-muted-foreground">Seat {i + 1}</span>
                <Select value={id} onValueChange={(v) => v && setLineup((l) => l.map((x, j) => (j === i ? String(v) : x)))}
                  items={items}>
                  <SelectTrigger size="sm" className="flex-1"><SelectValue /></SelectTrigger>
                  <SelectContent className="max-h-[22rem]">
                    {groups.filter(([, list]) => list.length > 0).map(([title, list], gi) => (
                      <SelectGroup key={title}>
                        {gi > 0 && <SelectSeparator />}
                        {title === 'Retired' && <SelectLabel>Retired</SelectLabel>}
                        {list.map((b) => (
                          <SelectItem key={b.id} value={b.id} title={b.retired ?? b.blurb}>
                            <span className={`flex w-full items-center justify-between gap-3 ${b.retired ? 'opacity-60' : ''}`}>
                              <span className="truncate">{b.name}</span>
                              <span className="tabular-nums text-muted-foreground">{elo.has(b.id) ? Math.round(elo.get(b.id)!) : '—'}</span>
                            </span>
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ))}
          </div>
        </div>

        <p className="text-[11px] text-muted-foreground">
          Every game ends the moment one player is left standing. From tick 3,000 attackers win each exchange more often (47% rising to 85%), and the cutoff is 10,000 ticks, where survivors are ranked by land. A game someone ends before the cutoff counts as much for Elo as 8 cutoff games in an 8-player lobby (N times in an N-player one).
        </p>

        <div className="grid grid-cols-2 gap-4">
          <div className="space-y-2">
            <Label htmlFor="seed">Seed</Label>
            <div className="flex gap-1">
              <Input id="seed" inputMode="numeric" placeholder="Random" value={seed} onChange={(e) => setSeed(e.target.value.replace(/[^0-9]/g, ''))} />
              <Button variant="outline" size="icon" onClick={() => setSeed(String(Math.floor(Math.random() * 1e9)))} aria-label="Random seed"><Dice5 /></Button>
            </div>
          </div>
          <div className="space-y-2">
            <Label>Games</Label>
            <ToggleGroup variant="outline" size="sm" value={[String(count)]} onValueChange={(v) => v.length && setCount(Number(v[v.length - 1]))}>
              {BATCHES.map((n) => <ToggleGroupItem key={n} value={String(n)} className="px-2.5 data-pressed:bg-foreground data-pressed:text-background">{n}</ToggleGroupItem>)}
            </ToggleGroup>
          </div>
        </div>
        {count > 1 && <p className="-mt-3 text-[11px] text-muted-foreground">Runs {count} games with consecutive seeds, so each map is different.</p>}

        {createError && <p className="text-sm text-destructive">{createError}</p>}
        <Button className="w-full" size="lg" onClick={create} disabled={creating || offline || bots.length === 0}>
          {creating ? <Loader2 className="animate-spin" /> : <Plus />}
          {count === 1 ? 'Create game' : `Create ${count} games`}
        </Button>
      </CardContent>
    </Card>
  )
})
