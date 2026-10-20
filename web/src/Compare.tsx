import { ArrowLeftRight, GitCompareArrows, Globe2, Loader2, Play, Trophy, TriangleAlert } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api } from './api'
import type { BotInfo, CompareFormat, Comparison, HeadToHead, Leaderboard, SharedGame, Tendencies } from './api'
import { BotAvatar } from './BotAvatar'
import { MAP_NAMES } from './model'
import { Page } from './Page'

const A_TEXT = 'text-sky-300'
const B_TEXT = 'text-orange-300'
const A_BAR = 'bg-sky-400/80'
const B_BAR = 'bg-orange-400/80'

const FORMAT_NAMES: Record<CompareFormat, string> = { duel: '1v1', ffa: '8-player', other: '3–7 players' }

function ago(ms: number) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return `${s}s ago`
  if (s < 3600) return `${Math.round(s / 60)}m ago`
  if (s < 86400) return `${Math.round(s / 3600)}h ago`
  return `${Math.round(s / 86400)}d ago`
}

const place = (p: { place_start: number; place_end: number }) =>
  p.place_start === p.place_end ? String(p.place_start) : `${p.place_start}–${p.place_end}`

function ending(g: SharedGame): string {
  const t = g.final_tick?.toLocaleString() ?? '?'
  switch (g.ending) {
    case 'conquest': return `Conquest at tick ${t}`
    case 'last_standing': return `Last one standing at tick ${t}`
    case 'cutoff': return 'Ran to the cutoff'
    case null: return ''
    default: { const never: never = g.ending; return never }
  }
}

type Metric = {
  key: keyof Tendencies
  label: string
  hint?: string
  fmt: (v: number) => string
  /** Which way is better, when one is. */
  better?: 'high' | 'low'
}

const one = (v: number) => v.toFixed(1)
const pct = (v: number) => `${Math.round(v)}%`
const SECTIONS: { title: string; blurb: string; metrics: Metric[] }[] = [
  {
    title: 'Results', blurb: 'Over every game under the current rules.', metrics: [
      { key: 'avg_place', label: 'Average place', hint: '1 is first', fmt: (v) => v.toFixed(2), better: 'low' },
      { key: 'win_pct', label: 'Won', fmt: pct, better: 'high' },
      { key: 'finish_pct', label: 'Won before the cutoff', hint: 'finished every rival off', fmt: pct, better: 'high' },
      { key: 'avg_land', label: 'Land at the end', hint: 'hexes', fmt: (v) => Math.round(v).toString(), better: 'high' },
      { key: 'avg_ticks', label: 'Game length', hint: 'ticks', fmt: (v) => Math.round(v).toLocaleString() },
      { key: 'ms_per_move', label: 'Thinking time', hint: 'ms per move', fmt: (v) => v.toFixed(v < 1 ? 3 : 2), better: 'low' },
    ],
  },
  {
    title: 'Economy', blurb: 'Buildings put up per game.', metrics: [
      { key: 'farms', label: 'Farms', fmt: one },
      { key: 'villages', label: 'Villages', fmt: one },
      { key: 'quarries', label: 'Quarries', fmt: one },
      { key: 'demolished', label: 'Demolished', hint: 'own buildings torn down', fmt: one },
    ],
  },
  {
    title: 'Walls', blurb: 'Walls go on borders and get stronger when linked.', metrics: [
      { key: 'walls', label: 'Walls per game', fmt: one },
      { key: 'wall_pct', label: 'Games with a Wall', fmt: pct },
      { key: 'first_wall', label: 'First Wall', hint: 'median tick', fmt: (v) => `tick ${Math.round(v).toLocaleString()}` },
    ],
  },
  {
    title: 'Fighting', blurb: 'Hexes per game.', metrics: [
      { key: 'captures', label: 'Hexes taken', hint: 'neutral or not', fmt: (v) => Math.round(v).toLocaleString() },
      { key: 'rival_captures', label: 'Taken from rivals', fmt: (v) => Math.round(v).toLocaleString() },
      { key: 'attack_success_pct', label: 'Attacks that land', fmt: (v) => `${v.toFixed(1)}%`, better: 'high' },
      { key: 'lost', label: 'Hexes lost to attacks', fmt: (v) => Math.round(v).toLocaleString(), better: 'low' },
      { key: 'sealed', label: 'Won by sealing regions', fmt: one, better: 'high' },
      { key: 'sealed_lost', label: 'Lost to rival seals', fmt: one, better: 'low' },
    ],
  },
]

function BotPicker({ value, onChange, bots, elo, exclude }: {
  value: string; onChange: (id: string) => void; bots: BotInfo[]; elo: Map<string, number>; exclude: string
}) {
  const items = useMemo(() => bots.map((b) => ({ value: b.id, label: b.name })), [bots])
  const groups = useMemo(() => {
    const byElo = (list: BotInfo[]) => [...list].sort((x, y) => (elo.get(y.id) ?? -Infinity) - (elo.get(x.id) ?? -Infinity) || x.name.localeCompare(y.name))
    return [['Active', byElo(bots.filter((b) => !b.retired))], ['Retired', byElo(bots.filter((b) => b.retired))]] as const
  }, [bots, elo])
  return (
    <Select value={value} onValueChange={(v) => v && onChange(String(v))} items={items}>
      <SelectTrigger size="sm" className="w-full"><SelectValue placeholder="Pick a bot" /></SelectTrigger>
      <SelectContent className="max-h-[22rem]">
        {groups.filter(([, list]) => list.length > 0).map(([title, list], gi) => (
          <SelectGroup key={title}>
            {gi > 0 && <SelectSeparator />}
            {title === 'Retired' && <SelectLabel>Retired</SelectLabel>}
            {list.map((b) => (
              <SelectItem key={b.id} value={b.id} disabled={b.id === exclude}>
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
  )
}

function Side({ id, name, elo, rank, tone, onBot }: { id: string; name: string; elo?: number; rank?: number; tone: string; onBot: (id: string) => void }) {
  return (
    <button type="button" onClick={() => onBot(id)} className="flex min-w-0 cursor-pointer items-center gap-3 rounded-xl p-1 text-left hover:bg-muted/40" title="Open profile">
      <BotAvatar id={id} name={name} size={56} className="shrink-0" />
      <div className="min-w-0">
        <div className={`truncate font-medium ${tone}`}>{name}</div>
        <div className="font-mono text-[11px] text-muted-foreground">{id}</div>
        <div className="text-xs text-muted-foreground">{elo != null ? `${Math.round(elo)} Elo${rank ? ` · #${rank}` : ''}` : 'unrated'}</div>
      </div>
    </button>
  )
}

function SplitBar({ a, b, even = 0 }: { a: number; b: number; even?: number }) {
  const total = a + b + even
  if (total === 0) return <div className="h-2 rounded-full bg-muted" />
  return (
    <div className="flex h-2 overflow-hidden rounded-full bg-muted">
      <div className={A_BAR} style={{ width: `${(100 * a) / total}%` }} />
      <div className="bg-muted-foreground/30" style={{ width: `${(100 * even) / total}%` }} />
      <div className={B_BAR} style={{ width: `${(100 * b) / total}%` }} />
    </div>
  )
}

function HeadToHeadRow({ format, r }: { format: CompareFormat; r: HeadToHead }) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-2 text-sm">
        <span className={`tabular-nums text-lg font-semibold ${A_TEXT}`}>{r.a_ahead}</span>
        <span className="text-xs text-muted-foreground">{FORMAT_NAMES[format]} · {r.games} game{r.games === 1 ? '' : 's'}{r.even ? ` · ${r.even} even` : ''}</span>
        <span className={`tabular-nums text-lg font-semibold ${B_TEXT}`}>{r.b_ahead}</span>
      </div>
      <SplitBar a={r.a_ahead} b={r.b_ahead} even={r.even} />
      <div className="flex justify-between text-[11px] text-muted-foreground">
        <span>{r.a_won} won outright</span>
        <span>{r.b_won} won outright</span>
      </div>
    </div>
  )
}

function MetricRow({ m, a, b }: { m: Metric; a?: number | null; b?: number | null }) {
  const has = (v?: number | null): v is number => v != null && Number.isFinite(v)
  const max = Math.max(has(a) ? Math.abs(a) : 0, has(b) ? Math.abs(b) : 0)
  const wins = (x?: number | null, y?: number | null) =>
    m.better && has(x) && has(y) && x !== y && (m.better === 'high' ? x > y : x < y)
  const cell = (v: number | null | undefined, bar: string, tone: string, mirrored: boolean, best: boolean) => (
    <div className={`flex items-center gap-2 ${mirrored ? 'flex-row-reverse' : ''}`}>
      <span className={`w-16 shrink-0 tabular-nums text-sm ${mirrored ? 'text-left' : 'text-right'} ${best ? `font-semibold ${tone}` : ''}`}>{has(v) ? m.fmt(v) : '—'}</span>
      <div className={`flex h-1.5 flex-1 ${mirrored ? '' : 'justify-end'}`}>
        <div className={`h-full rounded-full ${bar}`} style={{ width: has(v) && max > 0 ? `${(100 * Math.abs(v)) / max}%` : 0 }} />
      </div>
    </div>
  )
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_minmax(7rem,9rem)_minmax(0,1fr)] items-center gap-2 py-1.5 sm:gap-4">
      {cell(a, A_BAR, A_TEXT, false, !!wins(a, b))}
      <div className="text-center leading-tight">
        <div className="text-xs">{m.label}</div>
        {m.hint && <div className="text-[10px] text-muted-foreground">{m.hint}</div>}
      </div>
      {cell(b, B_BAR, B_TEXT, true, !!wins(b, a))}
    </div>
  )
}

export function ComparePage({ a, b, onBot, onOpen }: { a: string | null; b: string | null; onBot: (id: string) => void; onOpen: (id: string) => void }) {
  const [bots, setBots] = useState<BotInfo[]>([])
  const [board, setBoard] = useState<Leaderboard | null>(null)
  const [data, setData] = useState<Comparison | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [format, setFormat] = useState<'ffa' | 'duel'>('ffa')
  const [gamesFilter, setGamesFilter] = useState<'all' | CompareFormat>('all')

  useEffect(() => {
    api.bots().then(setBots).catch((e) => setError(String(e.message ?? e)))
    api.leaderboard().then(setBoard).catch(() => {})
  }, [])

  const set = (na: string | null, nb: string | null) => {
    const q = new URLSearchParams()
    if (na) q.set('a', na)
    if (nb) q.set('b', nb)
    location.replace(`#/compare?${q}`)
  }

  useEffect(() => {
    if ((!a || !b) && board && board.bots.length >= 2) {
      const top = board.bots.map((r) => r.bot)
      const na = a ?? top.find((x) => x !== b)!
      set(na, b ?? top.find((x) => x !== na)!)
    }
  }, [a, b, board])

  useEffect(() => {
    if (!a || !b) return
    let live = true
    setData(null); setError(null)
    api.compare(a, b)
      .then((d) => {
        if (!live) return
        setData(d)
        const games = (s: 'ffa' | 'duel') => (d.tendencies.a[s]?.games ?? 0) + (d.tendencies.b[s]?.games ?? 0)
        setFormat(games('ffa') >= games('duel') ? 'ffa' : 'duel')
      })
      .catch((e) => live && setError(String(e.message ?? e)))
    return () => { live = false }
  }, [a, b])

  const rows = useMemo(() => new Map([...(board?.bots ?? []), ...(board?.retired_bots ?? [])].map((r) => [r.bot, r])), [board])
  const elo = useMemo(() => new Map([...rows].map(([k, r]) => [k, r.elo])), [rows])
  const h2h = data?.head_to_head ?? {}
  const formats = (['duel', 'ffa', 'other'] as const).filter((f) => (h2h[f]?.games ?? 0) > 0)
  const total = formats.reduce((acc, f) => ({ a: acc.a + h2h[f]!.a_ahead, b: acc.b + h2h[f]!.b_ahead }), { a: 0, b: 0 })
  const ta = data?.tendencies.a[format]
  const tb = data?.tendencies.b[format]
  const shared = (data?.shared ?? []).filter((g) => gamesFilter === 'all' || (gamesFilter === 'duel' ? g.players === 2 : gamesFilter === 'ffa' ? g.players === 8 : g.players !== 2 && g.players !== 8))
  const partial = data && data.coverage.with_habits < data.coverage.games

  return (
    <Page icon={GitCompareArrows} wide title="Compare" description="Put two bots side by side: who finishes ahead when they meet, the games they've shared, and how each one plays.">
      <Card>
        <CardContent className="grid items-center gap-4 pt-6 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
          <div className="space-y-2">
            <BotPicker value={a ?? ''} onChange={(id) => set(id, b)} bots={bots} elo={elo} exclude={b ?? ''} />
            {data && <Side id={data.a.id} name={data.a.name} elo={rows.get(data.a.id)?.elo} rank={rows.get(data.a.id)?.rank} tone={A_TEXT} onBot={onBot} />}
          </div>
          <div className="flex justify-center">
            <Button variant="outline" size="icon" onClick={() => set(b, a)} disabled={!a || !b} aria-label="Swap the two bots" title="Swap"><ArrowLeftRight /></Button>
          </div>
          <div className="space-y-2">
            <BotPicker value={b ?? ''} onChange={(id) => set(a, id)} bots={bots} elo={elo} exclude={a ?? ''} />
            {data && <div className="flex justify-end"><Side id={data.b.id} name={data.b.name} elo={rows.get(data.b.id)?.elo} rank={rows.get(data.b.id)?.rank} tone={B_TEXT} onBot={onBot} /></div>}
          </div>
        </CardContent>
      </Card>

      {error && (
        <Card className="border-amber-500/40">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base"><TriangleAlert className="size-4 text-amber-400" />Couldn't compare these bots</CardTitle>
            <CardDescription>{error}</CardDescription>
          </CardHeader>
        </Card>
      )}
      {!data && !error && <div className="flex items-center justify-center gap-2 py-10 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading comparison…</div>}

      {data && (
        <>
          <div className="grid gap-6 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)]">
            <Card>
              <CardHeader>
                <CardTitle>Head to head</CardTitle>
                <CardDescription>
                  {data.shared_total === 0 ? `They haven't met under rules ${data.rules_version} yet.`
                    : `Who finished ahead in the ${data.shared_total} game${data.shared_total === 1 ? '' : 's'} they both played, rules ${data.rules_version}.`}
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-5">
                {data.shared_total === 0 ? (
                  <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
                    The arena seats bots with the fewest games first, so they'll likely meet soon. Or set up a game between them on the Games page.
                  </p>
                ) : (
                  <>
                    <div className="text-center">
                      <div className="text-4xl font-semibold tabular-nums"><span className={A_TEXT}>{total.a}</span><span className="px-2 text-muted-foreground">–</span><span className={B_TEXT}>{total.b}</span></div>
                      <div className="mt-1 text-xs text-muted-foreground">
                        {total.a === total.b ? 'Dead even' : `${total.a > total.b ? data.a.name : data.b.name} finishes ahead ${Math.round((100 * Math.max(total.a, total.b)) / Math.max(1, total.a + total.b))}% of the time`}
                      </div>
                    </div>
                    {formats.map((f) => <HeadToHeadRow key={f} format={f} r={h2h[f]!} />)}
                  </>
                )}
              </CardContent>
            </Card>

            <Card className="min-w-0">
              <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
                <div className="min-w-0 space-y-1.5">
                  <CardTitle>Tendencies</CardTitle>
                  <CardDescription>
                    Per-game averages over all their {FORMAT_NAMES[format]} games, not only the shared ones: {ta?.games ?? 0} for <span className={A_TEXT}>{data.a.name}</span>, {tb?.games ?? 0} for <span className={B_TEXT}>{data.b.name}</span>. The better side is bold where one is.
                  </CardDescription>
                </div>
                <ToggleGroup variant="outline" size="sm" value={[format]} onValueChange={(v) => v.length && setFormat(v[v.length - 1] as 'ffa' | 'duel')}>
                  <ToggleGroupItem value="ffa" className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">8-player</ToggleGroupItem>
                  <ToggleGroupItem value="duel" className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">1v1</ToggleGroupItem>
                </ToggleGroup>
              </CardHeader>
              <CardContent className="space-y-5">
                {partial && (
                  <p className="flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
                    <Loader2 className="size-3.5 animate-spin" />Building and fighting stats cover {data.coverage.with_habits} of {data.coverage.games} games so far; older replays are still being read.
                  </p>
                )}
                {!ta?.games && !tb?.games ? (
                  <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">Neither bot has a {FORMAT_NAMES[format]} game under these rules yet.</p>
                ) : SECTIONS.map((s) => (
                  <section key={s.title}>
                    <div className="mb-1 flex items-baseline gap-2 border-b pb-1">
                      <h3 className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">{s.title}</h3>
                      <span className="text-[11px] text-muted-foreground/70">{s.blurb}</span>
                    </div>
                    {s.metrics.map((m) => <MetricRow key={m.key} m={m} a={ta?.[m.key] as number | null | undefined} b={tb?.[m.key] as number | null | undefined} />)}
                  </section>
                ))}
              </CardContent>
            </Card>
          </div>

          <Card>
            <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-3">
              <div className="space-y-1.5">
                <CardTitle>Games together</CardTitle>
                <CardDescription>
                  {data.shared_total > data.shared.length ? `Newest ${data.shared.length} of ${data.shared_total}` : `${data.shared_total} game${data.shared_total === 1 ? '' : 's'}`}, newest first. Open one to watch the replay.
                </CardDescription>
              </div>
              {formats.length > 1 && (
                <ToggleGroup variant="outline" size="sm" value={[gamesFilter]} onValueChange={(v) => v.length && setGamesFilter(v[v.length - 1] as 'all' | CompareFormat)}>
                  <ToggleGroupItem value="all" className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">All</ToggleGroupItem>
                  {formats.map((f) => <ToggleGroupItem key={f} value={f} className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">{FORMAT_NAMES[f]}</ToggleGroupItem>)}
                </ToggleGroup>
              )}
            </CardHeader>
            <CardContent>
              {shared.length === 0 ? (
                <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No shared games{gamesFilter === 'all' ? '' : ` in ${FORMAT_NAMES[gamesFilter]}`} yet.</p>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead>Game</TableHead>
                      <TableHead className={A_TEXT}><span className="block max-w-28 truncate">{data.a.name}</span></TableHead>
                      <TableHead className={B_TEXT}><span className="block max-w-28 truncate">{data.b.name}</span></TableHead>
                      <TableHead className="hidden md:table-cell">How it ended</TableHead>
                      <TableHead className="text-right"><span className="sr-only">Watch</span></TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {shared.map((g) => {
                      const aAhead = g.a.place_start + g.a.place_end < g.b.place_start + g.b.place_end
                      const bAhead = g.b.place_start + g.b.place_end < g.a.place_start + g.a.place_end
                      const won = (p: SharedGame['a']) => p.place_start === 1 && p.place_end === 1
                      const placeCell = (p: SharedGame['a'], ahead: boolean, tone: string) => (
                        <TableCell className="tabular-nums">
                          <span className={`flex items-center gap-1 ${ahead ? `font-medium ${tone}` : 'text-muted-foreground'}`}>
                            {won(p) && <Trophy className="size-3.5 text-amber-300" />}{place(p)}<span className="text-muted-foreground font-normal">/{g.players}</span>
                          </span>
                          <div className="text-[11px] text-muted-foreground">{p.land} hexes</div>
                        </TableCell>
                      )
                      return (
                        <TableRow key={g.id}>
                          <TableCell>
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="font-mono text-xs">{g.id}</span>
                              <Badge variant="outline" className="text-[10px]">{g.players === 2 ? '1v1' : `${g.players}p`}</Badge>
                              {g.map && <Badge variant="outline" className="gap-1 border-sky-400/30 text-[10px] text-sky-300"><Globe2 className="size-3" />{MAP_NAMES[g.map]}</Badge>}
                            </div>
                            <div className="text-[11px] text-muted-foreground">{ago(g.created_at)}{g.auto ? ' · self-play' : ''}</div>
                          </TableCell>
                          {placeCell(g.a, aAhead, A_TEXT)}
                          {placeCell(g.b, bAhead, B_TEXT)}
                          <TableCell className="hidden text-xs text-muted-foreground md:table-cell">{ending(g)}</TableCell>
                          <TableCell className="text-right"><Button size="sm" variant="outline" onClick={() => onOpen(g.id)}><Play />Watch</Button></TableCell>
                        </TableRow>
                      )
                    })}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>
        </>
      )}
    </Page>
  )
}
