import { ArrowLeft, Gauge, Hash, Loader2, Play, Sprout, Swords, Trophy, TriangleAlert } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { BotProfile, EloCurve, Kin, ProfileGame } from './api'
import { api } from './api'
import { BotAvatar } from './BotAvatar'

const REFRESH_MS = 10_000
const CURVE_COLORS = ['#a78bfa', '#38bdf8', '#4ade80', '#facc15', '#fb923c', '#f472b6', '#f87171']

function ago(ms: number) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return `${s}s ago`
  if (s < 3600) return `${Math.round(s / 60)}m ago`
  if (s < 86400) return `${Math.round(s / 3600)}h ago`
  return `${Math.round(s / 86400)}d ago`
}

function ordinal(n: number) {
  const tens = n % 100
  if (tens >= 11 && tens <= 13) return `${n}th`
  return `${n}${['th', 'st', 'nd', 'rd'][n % 10] ?? 'th'}`
}

function place(g: ProfileGame) {
  if (g.place_start === null || g.place_end === null) return null
  return g.place_start === g.place_end ? ordinal(g.place_start) : `${ordinal(g.place_start)}–${ordinal(g.place_end)}`
}

function ending(g: ProfileGame) {
  switch (g.ending) {
    case 'conquest': return `conquest at tick ${g.final_tick}`
    case 'last_standing': return `last standing at tick ${g.final_tick}`
    case 'cutoff': return `cutoff at tick ${g.final_tick}`
    case null: return g.status === 'done' ? `ended at tick ${g.final_tick}` : g.status
    default: { const never: never = g.ending; return never }
  }
}

function Stat({ icon: Icon, label, value, hint }: { icon: typeof Hash; label: string; value: string; hint: string }) {
  return (
    <Card size="sm">
      <CardContent className="flex flex-col gap-1">
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground"><Icon className="size-3.5" />{label}</div>
        <div className="text-2xl font-semibold tabular-nums tracking-tight">{value}</div>
        <div className="text-xs text-muted-foreground">{hint}</div>
      </CardContent>
    </Card>
  )
}

const W = 720, H = 240, PAD = { l: 44, r: 16, t: 14, b: 28 }

function EloChart({ curves, start }: { curves: EloCurve[]; start: number }) {
  const series = curves
    .filter((c) => c.points.length > 0)
    .map((c) => ({ ...c, pts: [[c.points[0][0] - 1, start] as [number, number], ...c.points] }))
  if (series.length === 0) {
    return <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No rated games yet. The curve starts after its first finished game.</p>
  }
  const all = series.flatMap((s) => s.pts)
  const t0 = Math.min(...all.map((p) => p[0])), t1 = Math.max(...all.map((p) => p[0]))
  const lo = Math.min(start, ...all.map((p) => p[1])) - 20, hi = Math.max(start, ...all.map((p) => p[1])) + 20
  const x = (t: number) => PAD.l + ((t - t0) / Math.max(1, t1 - t0)) * (W - PAD.l - PAD.r)
  const y = (e: number) => H - PAD.b - ((e - lo) / (hi - lo)) * (H - PAD.t - PAD.b)
  const ticks = [lo + 20, start, hi - 20].filter((v, i, a) => a.findIndex((w) => Math.abs(w - v) < 25) === i)
  const time = (t: number) => new Date(t).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
  return (
    <div className="space-y-2">
      <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full" role="img" aria-label="Elo over time">
        {ticks.map((v) => (
          <g key={v}>
            <line x1={PAD.l} x2={W - PAD.r} y1={y(v)} y2={y(v)} stroke="currentColor" strokeOpacity={v === start ? 0.25 : 0.08} strokeDasharray={v === start ? '3 4' : undefined} />
            <text x={PAD.l - 6} y={y(v)} dy="0.32em" textAnchor="end" className="fill-muted-foreground text-[10px] tabular-nums">{Math.round(v)}</text>
          </g>
        ))}
        <text x={PAD.l} y={H - 8} className="fill-muted-foreground text-[10px]">{time(t0)}</text>
        <text x={W - PAD.r} y={H - 8} textAnchor="end" className="fill-muted-foreground text-[10px]">{time(t1)}</text>
        {series.map((s, i) => {
          const color = CURVE_COLORS[i % CURVE_COLORS.length]
          const last = s.pts[s.pts.length - 1]
          return (
            <g key={s.rules_version}>
              <polyline points={s.pts.map(([t, e]) => `${x(t).toFixed(1)},${y(e).toFixed(1)}`).join(' ')} fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" />
              <circle cx={x(last[0])} cy={y(last[1])} r={3.5} fill={color} />
            </g>
          )
        })}
      </svg>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
        {series.map((s, i) => (
          <span key={s.rules_version} className="flex items-center gap-1.5">
            <span className="size-2.5 rounded-full" style={{ background: CURVE_COLORS[i % CURVE_COLORS.length] }} />
            Rules {s.rules_version}: <span className="tabular-nums text-foreground">{Math.round(s.elo)}</span> after {s.games} game{s.games === 1 ? '' : 's'} (peak {Math.round(s.peak_elo)})
          </span>
        ))}
      </div>
    </div>
  )
}

function KinList({ title, hint, kin, empty, onBot }: { title: string; hint: string; kin: Kin[]; empty: string; onBot: (id: string) => void }) {
  return (
    <div className="min-w-0 space-y-2">
      <div>
        <div className="text-sm font-medium">{title}</div>
        <div className="text-xs text-muted-foreground">{hint}</div>
      </div>
      {kin.length === 0 ? (
        <p className="rounded-lg border border-dashed px-3 py-4 text-center text-xs text-muted-foreground">{empty}</p>
      ) : (
        <div className="flex flex-wrap gap-2">
          {kin.map((k, i) => (
            <button key={k.id} type="button" onClick={() => onBot(k.id)}
              className={`flex items-center gap-2 rounded-xl border bg-muted/30 py-1.5 pr-3 pl-1.5 text-left transition-colors hover:bg-muted ${k.retired ? 'opacity-60' : ''}`}
              title={k.retired ? `${k.name} (retired)` : k.name}>
              <BotAvatar id={k.id} name={k.name} size={36} />
              <span className="min-w-0">
                <span className="block truncate text-sm">{k.name}</span>
                <span className="block font-mono text-[10px] text-muted-foreground">{k.id}{title === 'Parents' && i === 0 && kin.length > 1 ? ' · main' : ''}{k.retired ? ' · retired' : ''}</span>
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

export function BotPage({ id, onBack, onOpen, onBot }: { id: string; onBack: () => void; onOpen: (match: string) => void; onBot: (id: string) => void }) {
  const [profile, setProfile] = useState<BotProfile | null>(null)
  const [error, setError] = useState<string | null>(null)
  const load = useCallback(() => {
    api.profile(id).then((p) => { setProfile(p); setError(null) }).catch((e) => setError(String(e.message ?? e)))
  }, [id])
  useEffect(() => {
    load()
    const t = setInterval(load, REFRESH_MS)
    return () => clearInterval(t)
  }, [load])

  const back = <Button variant="ghost" size="sm" onClick={onBack}><ArrowLeft />Arena</Button>
  if (error && !profile) {
    return (
      <div className="mx-auto max-w-5xl space-y-4 p-4 sm:p-6">
        {back}
        <Card className="border-amber-500/40">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base"><TriangleAlert className="size-4 text-amber-400" />Couldn't load this bot</CardTitle>
            <CardDescription>{error}</CardDescription>
          </CardHeader>
          <CardContent><Button size="sm" onClick={load}>Retry</Button></CardContent>
        </Card>
      </div>
    )
  }
  if (!profile) return <div className="flex h-full items-center justify-center gap-2 text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading bot…</div>

  const { bot, standing } = profile
  const row = standing?.row
  const finished = profile.games.filter((g) => g.status === 'done')
  const wins = finished.filter((g) => g.place_start === 1 && g.place_end === 1).length
  const author = bot.author?.startsWith('agent:') ? `agent · ${bot.author.slice(6)}` : bot.author
  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-5xl flex-col gap-6 p-4 sm:p-6">
        <div>{back}</div>

        <header className="flex flex-col items-center gap-4 text-center sm:flex-row sm:items-center sm:text-left">
          <BotAvatar id={bot.id} name={bot.name} size={120} className="shrink-0 drop-shadow-lg" />
          <div className="min-w-0 space-y-2">
            <h1 className="text-2xl font-semibold tracking-tight">{bot.name}</h1>
            <div className="flex flex-wrap justify-center gap-1.5 sm:justify-start">
              <Badge variant="outline" className="font-mono">{bot.id}</Badge>
              {bot.kind && <Badge variant="outline">{bot.kind === 'entrant' ? 'arena version' : 'roster CPU'}</Badge>}
              {author && <Badge className="bg-violet-500/20 text-violet-200">{author}</Badge>}
              {bot.brain && <Badge variant="outline">brain · {bot.brain}</Badge>}
              {bot.retired && <Badge variant="destructive" title={bot.retired}>retired</Badge>}
              {row && !row.rated && <Badge variant="outline" className="border-amber-400/40 text-amber-300">provisional</Badge>}
            </div>
            {bot.blurb && <p className="max-w-3xl text-sm text-muted-foreground">{bot.blurb}</p>}
            {bot.retired && <p className="text-xs text-red-300">{bot.retired}</p>}
          </div>
        </header>

        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Stat icon={Trophy} label={`Elo (rules ${profile.rules_version})`} value={row ? String(Math.round(row.elo)) : '—'}
            hint={row ? `${standing?.rank ? `rank ${standing.rank}` : 'retired'} · peak ${Math.round(row.peak_elo)}` : 'no games under these rules yet'} />
          <Stat icon={Hash} label="Games" value={row ? (row.rated ? String(row.games) : `${row.games}/${profile.rated_games}`) : '0'}
            hint={row ? `${row.wins} wins · top half ${Math.round(row.top_half_rate * 100)}%` : `${finished.length} arena games, ${wins} won`} />
          <Stat icon={Swords} label="AVP (8p)" value={row ? row.avp8.toFixed(2) : '—'} hint={row ? `±${row.avp8_se.toFixed(2)} · 1 is best, 8 is last` : 'average place, 8-player scale'} />
          <Stat icon={Gauge} label="Speed" value={row?.ms_per_move != null ? `${row.ms_per_move.toFixed(row.ms_per_move < 1 ? 3 : 2)} ms` : '—'}
            hint={row?.ms_per_move != null
              ? `per move · 95% CI ${row.ms_per_move_ci95 != null ? `±${row.ms_per_move_ci95.toFixed(row.ms_per_move < 1 ? 3 : 2)}` : 'needs 2 games'} · ${row.timed_games} timed game${row.timed_games === 1 ? '' : 's'}${row.slowest_ms != null ? ` · slowest ${row.slowest_ms.toFixed(1)} ms` : ''}`
              : 'timed from its next arena game'} />
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2"><Sprout className="size-4 text-emerald-400" />Family</CardTitle>
            <CardDescription>Parents are the bots this one takes its policy from, as its author declared when publishing. Kids inherit their mascot from their parents: ears from the main parent, a mix of the rest, and one feature of their own.</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-5 sm:grid-cols-2">
            <KinList title="Parents" hint="Where its policy comes from, main parent first" kin={profile.parents} onBot={onBot}
              empty={bot.kind === 'roster' ? 'A hand-written roster CPU: the first generation.' : 'Written from scratch, or its author hasn’t named its parents yet.'} />
            <KinList title="Kids" hint="Versions that build on this one" kin={profile.children} onBot={onBot} empty="No versions build on this one yet." />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Elo over time</CardTitle>
            <CardDescription>Rating after every game under rules {profile.rules_version}, from the {profile.start_elo} everyone starts at. Earlier rules versions aren't shown: ratings restart when the rules change.</CardDescription>
          </CardHeader>
          <CardContent><EloChart curves={profile.curves} start={profile.start_elo} /></CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Game log</CardTitle>
            <CardDescription>{profile.games.length === 0 ? `No arena games under rules ${profile.rules_version} yet.` : `${profile.games.length === 300 ? 'Newest 300' : profile.games.length} arena game${profile.games.length === 1 ? '' : 's'} under rules ${profile.rules_version}, newest first.`}</CardDescription>
          </CardHeader>
          <CardContent>
            {profile.games.length === 0 ? (
              <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">It hasn't played in the arena yet. The self-playing arena seats the bots with the fewest games first.</p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead>Game</TableHead>
                    <TableHead>Place</TableHead>
                    <TableHead className="hidden sm:table-cell">How it ended</TableHead>
                    <TableHead className="hidden text-right md:table-cell">ms/move</TableHead>
                    <TableHead className="hidden lg:table-cell">Opponents</TableHead>
                    <TableHead className="text-right"><span className="sr-only">Watch</span></TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {profile.games.map((g) => {
                    const p = place(g)
                    const won = g.place_start === 1 && g.place_end === 1
                    return (
                      <TableRow key={g.id}>
                        <TableCell>
                          <div className="font-mono text-xs">{g.id}</div>
                          <div className="text-[11px] text-muted-foreground">{ago(g.created_at)} · {g.rules_version}</div>
                        </TableCell>
                        <TableCell className="tabular-nums">
                          {g.status === 'running' ? <span className="flex items-center gap-1 text-muted-foreground"><Loader2 className="size-3 animate-spin" />playing</span>
                            : g.status === 'failed' ? <span className="text-destructive">failed</span>
                            : <span className={won ? 'flex items-center gap-1 font-medium text-amber-300' : ''}>{won && <Trophy className="size-3.5" />}{p ?? '—'}<span className="text-muted-foreground"> of {g.players}</span></span>}
                        </TableCell>
                        <TableCell className="hidden text-xs text-muted-foreground sm:table-cell">{g.status === 'done' ? ending(g) : ''}</TableCell>
                        <TableCell className="hidden text-right text-xs tabular-nums md:table-cell">{g.ms_per_move != null ? g.ms_per_move.toFixed(g.ms_per_move < 1 ? 3 : 2) : '—'}</TableCell>
                        <TableCell className="hidden max-w-80 truncate text-xs text-muted-foreground lg:table-cell" title={g.opponents.join(', ')}>{g.opponents.join(', ')}</TableCell>
                        <TableCell className="text-right">
                          <Button size="sm" variant="outline" disabled={g.status !== 'done'} onClick={() => onOpen(g.id)}><Play />Watch</Button>
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
