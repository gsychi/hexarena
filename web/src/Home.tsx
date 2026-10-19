import { Dice5, Dna, Loader2, Play, Plus, Shuffle, Trophy, TriangleAlert, Users } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { BotInfo, Leaderboard, MatchStatus, MatchSummary } from './api'
import { api } from './api'
import { PLAYER_COLORS } from './model'
import { Standings } from './Standings'

const BATCHES = [1, 5, 10, 20]
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

const BOT_GROUPS: [NonNullable<BotInfo['kind']>, string][] = [
  ['roster', 'Roster CPUs'],
  ['league', 'League generations'],
  ['entrant', 'Arena versions (agents and people)'],
]

export function Home({ onOpen, onLeague }: { onOpen: (id: string) => void; onLeague: () => void }) {
  const [bots, setBots] = useState<BotInfo[]>([])
  const [matches, setMatches] = useState<MatchSummary[] | null>(null)
  const [board, setBoard] = useState<Leaderboard | null>(null)
  const [offline, setOffline] = useState<string | null>(null)
  const [lineup, setLineup] = useState<string[]>(DEFAULT_LINEUP)
  const [seed, setSeed] = useState('')
  const [count, setCount] = useState(1)
  const [creating, setCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)

  const refresh = useCallback(() => {
    Promise.all([api.matches(), api.leaderboard()])
      .then(([m, l]) => { setMatches(m); setBoard(l); setOffline(null) })
      .catch((e) => setOffline(String(e.message ?? e)))
  }, [])

  useEffect(() => {
    api.bots().then(setBots).catch((e) => setOffline(String(e.message ?? e)))
    refresh()
  }, [refresh])

  const running = matches?.some((m) => m.status === 'running') ?? false
  useEffect(() => {
    if (!running) return
    const id = setInterval(refresh, 1200)
    return () => clearInterval(id)
  }, [running, refresh])

  const setPlayers = (n: number) => setLineup((l) => Array.from({ length: n }, (_, i) => l[i] ?? DEFAULT_LINEUP[i % DEFAULT_LINEUP.length]))
  const active = bots.filter((b) => !b.retired)
  const randomize = () => setLineup((l) => l.map(() => active[Math.floor(Math.random() * active.length)]?.id ?? 'expander'))
  const groups = BOT_GROUPS.map(([kind, title]) => [title, bots.filter((b) => (b.kind ?? 'roster') === kind)] as const).filter(([, list]) => list.length > 0)

  const create = async () => {
    setCreating(true); setCreateError(null)
    try {
      const s = seed.trim() === '' ? undefined : Number(seed)
      if (s !== undefined && (!Number.isSafeInteger(s) || s < 0)) throw new Error('Seed must be a whole number')
      const made = await api.create({ bots: lineup, seed: s, count })
      setMatches((m) => [...made, ...(m ?? [])])
    } catch (e) {
      setCreateError(e instanceof Error ? e.message : String(e))
    } finally {
      setCreating(false)
    }
  }

  return (
    <div className="mx-auto flex min-h-full max-w-6xl flex-col gap-6 p-4 sm:p-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <div className="text-[11px] font-semibold tracking-[0.3em] text-muted-foreground">HEXARENA</div>
          <h1 className="mt-1 text-2xl font-semibold tracking-tight">CPU arena</h1>
          <p className="text-sm text-muted-foreground">Set up a free-for-all, let the CPUs fight it out on the Rust engine, then watch the replay.</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={onLeague}><Dna />League progression</Button>
          <Button variant="outline" size="sm" onClick={() => onOpen('sample')}><Play />Watch sample match</Button>
        </div>
      </header>

      {offline && (
        <Card className="border-amber-500/40">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base"><TriangleAlert className="size-4 text-amber-400" />Game server isn't reachable</CardTitle>
            <CardDescription>
              Start it with <code className="rounded bg-muted px-1.5 py-0.5 text-foreground">cargo run --release -p hexarena-server</code>, then this page reconnects. You can still watch the sample match.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex gap-2">
            <Button size="sm" onClick={refresh}>Retry</Button>
            <span className="self-center text-xs text-muted-foreground">{offline}</span>
          </CardContent>
        </Card>
      )}

      <Standings board={board} />

      <div className="grid gap-6 lg:grid-cols-[minmax(0,26rem)_minmax(0,1fr)]">
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
                      items={bots.map((b) => ({ value: b.id, label: b.name }))}>
                      <SelectTrigger size="sm" className="flex-1"><SelectValue /></SelectTrigger>
                      <SelectContent className="max-h-[22rem]">
                        {groups.map(([title, list], gi) => (
                          <SelectGroup key={title}>
                            {gi > 0 && <SelectSeparator />}
                            <SelectLabel>{title}</SelectLabel>
                            {list.map((b) => (
                              <SelectItem key={b.id} value={b.id}>
                                <div className={`flex flex-col ${b.retired ? 'opacity-60' : ''}`}>
                                  <span>
                                    {b.name}
                                    {b.kind === 'entrant' && <span className="ml-1.5 text-[10px] uppercase tracking-wide text-violet-300">{b.author}</span>}
                                    {b.retired && <span className="ml-1.5 text-[10px] uppercase tracking-wide text-red-300">retired</span>}
                                  </span>
                                  <span className="text-[11px] text-muted-foreground">{b.retired ?? b.blurb}</span>
                                </div>
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
              Every game ends the moment one player is left standing, with a cutoff at 3000 ticks. At the cutoff, survivors are ranked by land.
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
            <Button className="w-full" size="lg" onClick={create} disabled={creating || !!offline || bots.length === 0}>
              {creating ? <Loader2 className="animate-spin" /> : <Plus />}
              {count === 1 ? 'Create game' : `Create ${count} games`}
            </Button>
          </CardContent>
        </Card>

        <div className="flex min-w-0 flex-col gap-6">
          <Card>
            <CardHeader>
              <CardTitle>Games</CardTitle>
              <CardDescription>{matches ? `${matches.length} game${matches.length === 1 ? '' : 's'}, newest first.` : 'Loading…'}</CardDescription>
            </CardHeader>
            <CardContent>
              {matches === null && !offline && <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading games…</div>}
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
                        {m.winner && <span className="flex items-center gap-1 text-xs"><Trophy className="size-3 text-amber-300" />{m.winner}</span>}
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
      </div>
    </div>
  )
}
