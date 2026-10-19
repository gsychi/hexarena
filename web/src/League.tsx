import { ArrowLeft, Dna, FlaskConical, Gauge, Loader2, Play, Sparkles, Swords, TriangleAlert } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { Attempt, Generation, League } from './api'
import { api } from './api'

const REFRESH_MS = 4000
const ROSTER = ['expander', 'homesteader', 'raider', 'warden']

function ago(ms: number) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return `${s}s ago`
  if (s < 3600) return `${Math.round(s / 60)}m ago`
  if (s < 86400) return `${Math.round(s / 3600)}h ago`
  return `${Math.round(s / 86400)}d ago`
}

function duration(secs: number) {
  if (secs < 90) return `${Math.round(secs)}s`
  if (secs < 5400) return `${Math.round(secs / 60)}m`
  return `${(secs / 3600).toFixed(1)}h`
}

function originLabel(origin: string) {
  return origin.startsWith('agent:') ? `agent · ${origin.slice(6)}` : origin
}

function OriginBadge({ origin }: { origin: string }) {
  const agent = origin.startsWith('agent:')
  return (
    <Badge variant={agent ? 'default' : 'outline'} className={agent ? 'bg-violet-500/20 text-violet-200' : ''}>
      {originLabel(origin)}
    </Badge>
  )
}

function Stat({ icon: Icon, label, value, hint }: { icon: typeof Dna; label: string; value: string; hint: string }) {
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

interface Series { x: number; y: number; label: string }

const W = 560, H = 220, PAD = { l: 40, r: 14, t: 12, b: 26 }

/** A line chart over generations. `invert` puts low values at the top (AVP: lower is better). */
function LineChart({ points, lo, hi, invert, color, ticks, format }: {
  points: Series[]; lo: number; hi: number; invert?: boolean; color: string; ticks: number[]; format: (v: number) => string
}) {
  const maxX = Math.max(1, ...points.map((p) => p.x))
  const x = (v: number) => PAD.l + (v / maxX) * (W - PAD.l - PAD.r)
  const t = (v: number) => (v - lo) / (hi - lo)
  const y = (v: number) => PAD.t + (invert ? t(v) : 1 - t(v)) * (H - PAD.t - PAD.b)
  const path = points.map((p, i) => `${i ? 'L' : 'M'}${x(p.x).toFixed(1)},${y(p.y).toFixed(1)}`).join(' ')
  const xTicks = Array.from(new Set([0, Math.round(maxX / 2), maxX]))
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full" role="img">
      {ticks.map((v) => (
        <g key={v}>
          <line x1={PAD.l} x2={W - PAD.r} y1={y(v)} y2={y(v)} stroke="currentColor" strokeOpacity={0.1} />
          <text x={PAD.l - 6} y={y(v) + 3.5} textAnchor="end" className="fill-muted-foreground text-[10px] tabular-nums">{format(v)}</text>
        </g>
      ))}
      {xTicks.map((v) => (
        <text key={v} x={x(v)} y={H - 8} textAnchor="middle" className="fill-muted-foreground text-[10px] tabular-nums">g{v}</text>
      ))}
      <path d={path} fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" />
      {points.map((p) => (
        <circle key={p.x} cx={x(p.x)} cy={y(p.y)} r={3.2} fill={color} stroke="var(--background)" strokeWidth={1.2}>
          <title>{p.label}</title>
        </circle>
      ))}
    </svg>
  )
}

function AttemptsChart({ attempts, minGain }: { attempts: Attempt[]; minGain: number }) {
  if (attempts.length === 0) {
    return <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No challengers gated yet. The first one is playing now.</p>
  }
  const first = attempts[0].n, last = attempts[attempts.length - 1].n
  const gains = attempts.map((a) => a.gate.gain)
  const span = Math.max(0.5, ...gains.map(Math.abs))
  const lo = -Math.min(3, span) - 0.1, hi = Math.min(3, span) + 0.1
  const clamp = (v: number) => Math.max(lo, Math.min(hi, v))
  const x = (n: number) => PAD.l + ((n - first) / Math.max(1, last - first)) * (W - PAD.l - PAD.r)
  const y = (v: number) => PAD.t + (1 - (clamp(v) - lo) / (hi - lo)) * (H - PAD.t - PAD.b)
  const step = hi > 2 ? 1 : 0.5
  const ticks: number[] = []
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) ticks.push(Number(v.toFixed(2)))
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full" role="img">
      {ticks.map((v) => (
        <g key={v}>
          <line x1={PAD.l} x2={W - PAD.r} y1={y(v)} y2={y(v)} stroke="currentColor" strokeOpacity={v === 0 ? 0.35 : 0.1} />
          <text x={PAD.l - 6} y={y(v) + 3.5} textAnchor="end" className="fill-muted-foreground text-[10px] tabular-nums">{v > 0 ? `+${v}` : v}</text>
        </g>
      ))}
      <line x1={PAD.l} x2={W - PAD.r} y1={y(minGain)} y2={y(minGain)} stroke="#4ade80" strokeOpacity={0.35} strokeDasharray="3 4" />
      <text x={PAD.l} y={H - 8} className="fill-muted-foreground text-[10px] tabular-nums">#{first}</text>
      <text x={W - PAD.r} y={H - 8} textAnchor="end" className="fill-muted-foreground text-[10px] tabular-nums">#{last}</text>
      {attempts.map((a) => {
        const promoted = a.became !== null
        const color = promoted ? '#4ade80' : a.origin.startsWith('agent:') ? '#a78bfa' : '#94a3b8'
        return (
          <g key={a.n}>
            {promoted && <line x1={x(a.n)} x2={x(a.n)} y1={y(a.gate.gain - 2 * a.gate.gain_se)} y2={y(a.gate.gain + 2 * a.gate.gain_se)} stroke={color} strokeOpacity={0.6} />}
            <circle cx={x(a.n)} cy={y(a.gate.gain)} r={promoted ? 4.5 : 2.6} fill={color} fillOpacity={promoted ? 1 : 0.6}>
              <title>{`${a.id} (${originLabel(a.origin)}): ${a.gate.gain >= 0 ? '+' : ''}${a.gate.gain.toFixed(2)} ± ${a.gate.gain_se.toFixed(2)} over ${a.gate.games} games${promoted ? ` — became g${a.became}` : ''}\n${a.changes.join(', ')}`}</title>
            </circle>
          </g>
        )
      })}
    </svg>
  )
}

function Legend({ items }: { items: [string, string][] }) {
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
      {items.map(([color, label]) => (
        <span key={label} className="inline-flex items-center gap-1.5"><span className="size-2.5 rounded-full" style={{ background: color }} />{label}</span>
      ))}
    </div>
  )
}

function Changes({ changes }: { changes: string[] }) {
  if (changes.length === 0) return <span className="text-muted-foreground">—</span>
  return (
    <div className="flex flex-wrap gap-1">
      {changes.map((c) => <code key={c} className="rounded bg-muted px-1.5 py-0.5 text-[11px]">{c}</code>)}
    </div>
  )
}

function GenerationRow({ g, elo, champion, onPlay, playing }: { g: Generation; elo: number; champion: boolean; onPlay: () => void; playing: boolean }) {
  return (
    <TableRow className="hover:bg-muted/30">
      <TableCell className="min-w-[11rem] whitespace-normal">
        <div className="flex items-center gap-1.5 font-semibold">{g.name || g.id}{champion && <Sparkles className="size-3.5 shrink-0 text-amber-300" />}</div>
        <div className="mt-0.5 text-xs text-muted-foreground">
          {g.parents === undefined ? null : g.parents.length ? `Parents: ${g.parents.join(' × ')}` : 'No parents: written from scratch'}
          {g.beat && <> · beat {g.beat}</>}
        </div>
      </TableCell>
      <TableCell className="hidden sm:table-cell"><OriginBadge origin={g.origin} /></TableCell>
      <TableCell className="max-w-[22rem] whitespace-normal">
        <Changes changes={g.changes} />
        {g.note && <div className="mt-1 text-xs text-muted-foreground">{g.note}</div>}
      </TableCell>
      <TableCell className="hidden text-right tabular-nums md:table-cell">
        {g.gate ? <span className="text-emerald-400">+{g.gate.gain.toFixed(2)} <span className="text-muted-foreground">± {g.gate.gain_se.toFixed(2)}</span></span> : '—'}
      </TableCell>
      <TableCell className="text-right font-medium tabular-nums">{g.benchmark.avp.toFixed(2)}</TableCell>
      <TableCell className="hidden text-right tabular-nums sm:table-cell">{Math.round(g.benchmark.win_rate * 100)}%</TableCell>
      <TableCell className="hidden text-right tabular-nums lg:table-cell">{Math.round(g.benchmark.top_half_rate * 100)}%</TableCell>
      <TableCell className="hidden text-right tabular-nums md:table-cell">
        {g.benchmark.close_rate === undefined ? '—' : `${Math.round(g.benchmark.close_rate * 100)}%`}
        {g.benchmark.avg_close_tick != null && <div className="text-[11px] text-muted-foreground">by t{Math.round(g.benchmark.avg_close_tick)}</div>}
      </TableCell>
      <TableCell className="text-right tabular-nums">{Math.round(elo)}</TableCell>
      <TableCell className="hidden text-right text-xs text-muted-foreground lg:table-cell">{ago(g.created_at)}</TableCell>
      <TableCell className="text-right">
        <Button size="xs" variant="outline" onClick={onPlay} disabled={playing} aria-label={`Play ${g.id} against the roster`}>
          {playing ? <Loader2 className="animate-spin" /> : <Play />}Play
        </Button>
      </TableCell>
    </TableRow>
  )
}

export function LeaguePage({ onBack, onOpen }: { onBack: () => void; onOpen: (id: string) => void }) {
  const [league, setLeague] = useState<League | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [playing, setPlaying] = useState<string | null>(null)
  const [playError, setPlayError] = useState<string | null>(null)

  const refresh = useCallback(() => {
    api.league()
      .then((l) => { setLeague(l); setError(null) })
      .catch((e) => setError(String(e.message ?? e)))
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    refresh()
    const id = setInterval(refresh, REFRESH_MS)
    return () => clearInterval(id)
  }, [refresh])

  const play = async (gen: string) => {
    setPlaying(gen); setPlayError(null)
    try {
      const lineup = [gen, ...Array.from({ length: 7 }, (_, i) => ROSTER[i % ROSTER.length])]
      const [m] = await api.create({ bots: lineup, count: 1 })
      for (;;) {
        const s = await api.match(m.id)
        if (s.status === 'done') { onOpen(s.id); return }
        if (s.status === 'failed') throw new Error(s.error ?? 'match failed')
        await new Promise((r) => setTimeout(r, 700))
      }
    } catch (e) {
      setPlayError(e instanceof Error ? e.message : String(e))
    } finally {
      setPlaying(null)
    }
  }

  const view = useMemo(() => {
    if (!league) return null
    const gens = league.generations
    const champ = gens[gens.length - 1]
    const eloOf = (g: Generation) => league.elo[g.id] ?? g.elo
    const avp = gens.map((g) => ({ x: g.gen, y: g.benchmark.avp, label: `${g.id}: benchmark AVP ${g.benchmark.avp.toFixed(2)}` }))
    const elo = gens.map((g) => ({ x: g.gen, y: eloOf(g), label: `${g.id}: Elo ${Math.round(eloOf(g))}` }))
    const elos = elo.map((p) => p.y)
    const eloLo = Math.floor((Math.min(1500, ...elos) - 20) / 50) * 50
    const eloHi = Math.ceil((Math.max(1500, ...elos) + 20) / 50) * 50
    const eloStep = Math.max(50, Math.ceil((eloHi - eloLo) / 4 / 50) * 50)
    const eloTicks: number[] = []
    for (let v = eloLo; v <= eloHi; v += eloStep) eloTicks.push(v)
    const recent = [...league.attempts].reverse().slice(0, 10)
    const promotedCount = league.attempts.filter((a) => a.became !== null).length
    const rate = league.play_secs > 0 ? league.games_total / league.play_secs : 0
    const roster = Object.entries(league.elo).filter(([id]) => id.startsWith('r-')).sort((a, b) => b[1] - a[1])
    return { gens, champ, eloOf, avp, elo, eloLo, eloHi, eloTicks, recent, promotedCount, rate, roster }
  }, [league])

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex min-h-full max-w-6xl flex-col gap-6 p-4 sm:p-6">
        <header className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <Button variant="ghost" size="xs" className="-ml-2 mb-1 text-muted-foreground" onClick={onBack}><ArrowLeft />CPU arena</Button>
            <div className="text-[11px] font-semibold tracking-[0.3em] text-muted-foreground">HEXARENA</div>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight">League</h1>
            <p className="max-w-2xl text-sm text-muted-foreground">
              CPU genomes improving themselves. Each challenger plays the champion in mirrored 8-player lobbies and is promoted only when it places significantly better.
              Every generation is then benchmarked against the hand-written roster on the same fixed seeds.
            </p>
          </div>
          {view && (
            <Button size="sm" onClick={() => play(view.champ.id)} disabled={playing !== null}>
              {playing === view.champ.id ? <Loader2 className="animate-spin" /> : <Swords />}Watch champion {view.champ.id} vs the roster
            </Button>
          )}
        </header>

        {playError && <p className="text-sm text-red-400">Couldn't start that game: {playError}</p>}

        {loading && !league && (
          <div className="flex items-center gap-2 py-16 text-sm text-muted-foreground justify-center"><Loader2 className="size-4 animate-spin" />Loading the league…</div>
        )}

        {error && !league && (
          <Card className="border-amber-500/40">
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base"><TriangleAlert className="size-4 text-amber-400" />No league to show</CardTitle>
              <CardDescription>
                {error.includes('no league') ? (
                  <>Start one with <code className="rounded bg-muted px-1.5 py-0.5 text-foreground">cargo run --release -p hexarena-cli -- league run</code> and this page fills in as generations are promoted.</>
                ) : (
                  <>The game server isn't reachable. Start it with <code className="rounded bg-muted px-1.5 py-0.5 text-foreground">cargo run --release -p hexarena-server</code>.</>
                )}
              </CardDescription>
            </CardHeader>
            <CardContent className="flex gap-2">
              <Button size="sm" onClick={refresh}>Retry</Button>
              <span className="self-center text-xs text-muted-foreground">{error}</span>
            </CardContent>
          </Card>
        )}

        {league && view && (
          <>
            <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
              <Stat icon={Sparkles} label="Champion" value={view.champ.id} hint={`${view.champ.name ? `${view.champ.name} · ` : ''}benchmark AVP ${view.champ.benchmark.avp.toFixed(2)} · Elo ${Math.round(view.eloOf(view.champ))}`} />
              <Stat icon={Dna} label="Generations" value={String(view.gens.length - 1)} hint={`promoted from ${league.attempts_total} challengers`} />
              <Stat icon={FlaskConical} label="Games played" value={league.games_total.toLocaleString()} hint={`8-player lobbies · ${duration(league.play_secs)} of play`} />
              <Stat icon={Gauge} label="Throughput" value={`${view.rate.toFixed(1)}/s`} hint={`games per second · updated ${ago(league.updated_at)}`} />
            </div>

            <div className="grid gap-4 lg:grid-cols-2">
              <Card>
                <CardHeader>
                  <CardTitle>Benchmark AVP by generation</CardTitle>
                  <CardDescription>
                    Average place on the 8-player scale (1 = always first, 8 = always last) over {view.champ.benchmark.games} fixed lobbies: two seats for the generation, six for the hand-written roster. Lower is better, so up is better.
                  </CardDescription>
                </CardHeader>
                <CardContent>
                  <LineChart points={view.avp} lo={1} hi={8} invert color="#38bdf8" ticks={[1, 2, 3, 4, 5, 6, 7, 8]} format={(v) => String(v)} />
                </CardContent>
              </Card>
              <Card>
                <CardHeader>
                  <CardTitle>Elo by generation</CardTitle>
                  <CardDescription>
                    Current multiplayer Elo from every league game (K = 32, start 1500). Older generations keep playing as sparring partners, so their ratings keep moving.
                  </CardDescription>
                </CardHeader>
                <CardContent>
                  <LineChart points={view.elo} lo={view.eloLo} hi={view.eloHi} color="#fbbf24" ticks={view.eloTicks} format={(v) => String(v)} />
                </CardContent>
              </Card>
            </div>

            <Card>
              <CardHeader>
                <CardTitle>Challengers</CardTitle>
                <CardDescription>
                  Gain is how many places better the challenger finished than the champion in the same lobbies (positive is better). Scores are 8-player places, minus a bonus for ending the game early ({league.gate_config.close_bonus ?? 0} places, plus up to {league.gate_config.close_speed_bonus ?? 0} more the earlier it ends). A challenger is promoted at +{league.gate_config.min_gain} or more when the gain clears {league.gate_config.z_accept} standard errors.
                  Gates stop early once the outcome is clear: {league.gate_config.min_pairs * 2}–{league.gate_config.max_pairs * 2} games each. {view.promotedCount} of the last {league.attempts.length} were promoted.
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-4">
                <AttemptsChart attempts={league.attempts} minGain={league.gate_config.min_gain} />
                <Legend items={[['#4ade80', 'Promoted (bar: ±2 SE)'], ['#94a3b8', 'Rejected mutation or crossover'], ['#a78bfa', 'Rejected agent submission']]} />
                {view.recent.length > 0 && (
                  <div className="divide-y rounded-lg border">
                    {view.recent.map((a) => (
                      <div key={a.n} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2 text-sm">
                        <span className="w-12 font-medium tabular-nums">{a.id}</span>
                        <OriginBadge origin={a.origin} />
                        <span className={`w-28 tabular-nums ${a.became !== null ? 'text-emerald-400' : a.gate.gain < 0 ? 'text-red-400' : 'text-muted-foreground'}`}>
                          {a.gate.gain >= 0 ? '+' : ''}{a.gate.gain.toFixed(2)} ± {a.gate.gain_se.toFixed(2)}
                        </span>
                        <div className="min-w-0 flex-1"><Changes changes={a.changes} /></div>
                        <span className="text-xs text-muted-foreground tabular-nums">{a.gate.games} games · {a.gate.secs.toFixed(0)}s</span>
                        {a.became !== null ? <Badge className="bg-emerald-500/20 text-emerald-300">became g{a.became}</Badge> : <Badge variant="outline" className="text-muted-foreground">rejected</Badge>}
                      </div>
                    ))}
                  </div>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle>Generations</CardTitle>
                <CardDescription>Every promoted genome, newest first, named by brain and lineage: "← g2" lists its parents, and a new brain written from scratch has none. Changes are the genes that differ from the champion it beat. Closes is the share of benchmark games it won by eliminating everyone before the cutoff. Play any of them against the roster in an 8-player game and watch the replay.</CardDescription>
              </CardHeader>
              <CardContent>
                <Table>
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead>Generation and lineage</TableHead>
                      <TableHead className="hidden sm:table-cell">Origin</TableHead>
                      <TableHead>Changes</TableHead>
                      <TableHead className="hidden text-right md:table-cell">Gate gain</TableHead>
                      <TableHead className="text-right">Bench AVP</TableHead>
                      <TableHead className="hidden text-right sm:table-cell">Win %</TableHead>
                      <TableHead className="hidden text-right lg:table-cell">Top half %</TableHead>
                      <TableHead className="hidden text-right md:table-cell">Closes</TableHead>
                      <TableHead className="text-right">Elo</TableHead>
                      <TableHead className="hidden text-right lg:table-cell">Promoted</TableHead>
                      <TableHead />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {[...view.gens].reverse().map((g) => (
                      <GenerationRow key={g.id} g={g} elo={view.eloOf(g)} champion={g.id === view.champ.id} onPlay={() => play(g.id)} playing={playing === g.id} />
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>

            {view.roster.length > 0 && (
              <Card size="sm">
                <CardHeader>
                  <CardTitle className="text-sm">Hand-written roster in the league</CardTitle>
                  <CardDescription>The original CPUs fill benchmark and sparring seats, so their Elo is the fixed yardstick the generations climb past.</CardDescription>
                </CardHeader>
                <CardContent className="flex flex-wrap gap-2">
                  {view.roster.map(([id, elo]) => (
                    <Badge key={id} variant="outline" className="gap-1.5 tabular-nums">{id.slice(2)}<span className="text-muted-foreground">{Math.round(elo)}</span></Badge>
                  ))}
                </CardContent>
              </Card>
            )}
          </>
        )}
      </div>
    </div>
  )
}
