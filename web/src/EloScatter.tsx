import { useState } from 'react'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { ArenaEntrant, LeaderboardRow } from './api'

type Axis = 'published' | 'speed'

interface Point { id: string; x: number; elo: number; author: string | null; retired: boolean; label: string; frontier?: boolean }

const W = 960, H = 340, PAD = { l: 46, r: 150, t: 14, b: 28 }
const AUTHOR_COLORS = ['#a78bfa', '#38bdf8', '#4ade80', '#facc15', '#fb923c', '#f472b6', '#f87171', '#2dd4bf']

function clock(ms: number, spanMs: number) {
  const d = new Date(ms)
  if (spanMs > 36 * 3600_000) return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

/**
 * Every arena version's Elo against when it was published, with the frontier: the best
 * rating anything published so far has reached. Switched to speed, the x axis is
 * milliseconds per move (log scale) and the frontier is the Pareto front: no other bot
 * is both faster and stronger. Roster CPUs have no publish date, so they are dashed
 * reference lines against time and ordinary points against speed.
 */
export function EloScatter({ rows, retiredRows, entrants, top, onBot }: { rows: LeaderboardRow[]; retiredRows: LeaderboardRow[]; entrants: ArenaEntrant[]; top: number; onBot: (id: string) => void }) {
  const [axis, setAxis] = useState<Axis>('published')
  const rated = new Map([...rows, ...retiredRows].map((r) => [r.bot, r]))
  const authors = [...new Set(entrants.map((e) => e.author))].sort()
  const color = (author: string | null) => (author === null ? '#94a3b8' : AUTHOR_COLORS[authors.indexOf(author) % AUTHOR_COLORS.length])
  const points: Point[] = []
  for (const e of entrants) {
    const r = rated.get(e.id)
    if (!r) continue
    const x = axis === 'published' ? e.created_at : r.ms_per_move
    if (x == null || x <= 0) continue
    points.push({
      id: e.id, x, elo: r.elo, author: e.author, retired: e.retired,
      label: `${r.name} (${e.id}) by ${e.author}\nElo ${Math.round(r.elo)} over ${r.games} games${r.ms_per_move != null ? `\n${r.ms_per_move.toFixed(2)} ms/move` : ''}${e.retired ? '\nretired' : ''}`,
    })
  }
  const roster = [...rows, ...retiredRows].filter((r) => !entrants.some((e) => e.id === r.bot))
  if (axis === 'speed') {
    for (const r of roster) {
      if (r.ms_per_move == null || r.ms_per_move <= 0) continue
      points.push({ id: r.bot, x: r.ms_per_move, elo: r.elo, author: null, retired: !rows.includes(r), label: `${r.name} (roster CPU)\nElo ${Math.round(r.elo)} over ${r.games} games\n${r.ms_per_move.toFixed(2)} ms/move` })
    }
  }
  const cut = rows.length >= top ? rows[top - 1].elo : null

  const toggle = (
    <ToggleGroup variant="outline" size="sm" value={[axis]} onValueChange={(v) => v.length && setAxis(v[v.length - 1] as Axis)}>
      <ToggleGroupItem value="published" className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">Elo vs time published</ToggleGroupItem>
      <ToggleGroupItem value="speed" className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">Elo vs ms per move</ToggleGroupItem>
    </ToggleGroup>
  )
  if (points.length === 0) {
    return (
      <div className="space-y-3">
        {toggle}
        <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No rated versions yet. Points appear after their first games under these rules.</p>
      </div>
    )
  }

  const steps: [number, number][] = []
  let best = -Infinity
  if (axis === 'published') {
    for (const p of [...points].sort((a, b) => a.x - b.x)) {
      if (p.elo > best) { best = p.elo; p.frontier = true; steps.push([p.x, p.elo]) }
    }
  } else {
    for (const p of [...points].sort((a, b) => a.x - b.x || b.elo - a.elo)) {
      if (p.elo > best) { best = p.elo; p.frontier = true; steps.push([p.x, p.elo]) }
    }
  }
  const now = Date.now()
  const xs = points.map((p) => p.x)
  const log = axis === 'speed'
  const fx = (v: number) => (log ? Math.log10(v) : v)
  const x0 = log ? Math.floor(fx(Math.min(...xs)) * 2) / 2 : Math.min(...xs)
  const x1 = log ? Math.ceil(fx(Math.max(...xs)) * 2) / 2 : Math.max(now, x0 + 60_000)
  const elos = [...points.map((p) => p.elo), ...roster.map((r) => r.elo), ...(cut ? [cut] : [])]
  const lo = Math.floor((Math.min(...elos) - 30) / 50) * 50, hi = Math.ceil((Math.max(...elos) + 30) / 50) * 50
  const x = (v: number) => PAD.l + ((fx(v) - x0) / Math.max(1e-9, x1 - x0)) * (W - PAD.l - PAD.r)
  const y = (v: number) => PAD.t + (1 - (v - lo) / (hi - lo)) * (H - PAD.t - PAD.b)
  const yStep = hi - lo > 600 ? 200 : hi - lo > 300 ? 100 : 50
  const yTicks: number[] = []
  for (let v = lo; v <= hi; v += yStep) yTicks.push(v)
  const xTicks = log
    ? Array.from({ length: Math.round((x1 - x0) * 2) + 1 }, (_, i) => 10 ** (x0 + i / 2))
    : [0, 0.25, 0.5, 0.75, 1].map((f) => x0 + f * (x1 - x0))
  const xLabel = (v: number) => (log ? `${v >= 10 ? Math.round(v) : v >= 1 ? v.toFixed(1) : v.toFixed(2)} ms` : clock(v, x1 - x0))
  const end = log ? x(10 ** x1) : x(now)
  const frontier = steps.map(([t, e], i) => `${i ? `H${x(t).toFixed(1)}V` : `M${x(t).toFixed(1)},`}${y(e).toFixed(1)}`).join('') + `H${end.toFixed(1)}`
  const labels = [
    { key: 'frontier', text: `${log ? 'Pareto' : 'frontier'} ${Math.round(best)}`, y: y(best), cls: 'fill-emerald-300 font-medium' },
    ...(cut !== null ? [{ key: 'cut', text: `top ${top} cut ${Math.round(cut)}`, y: y(cut), cls: 'fill-red-300' }] : []),
    ...(log ? [] : roster).map((r) => ({ key: r.bot, text: `${r.name} ${Math.round(r.elo)}`, y: y(r.elo), cls: 'fill-muted-foreground' })),
  ].sort((a, b) => a.y - b.y)
  for (let i = 1; i < labels.length; i++) labels[i].y = Math.max(labels[i].y, labels[i - 1].y + 11)

  return (
    <div className="space-y-3">
      {toggle}
      <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full" role="img" aria-label={log ? 'Elo of every version against its time per move' : 'Elo of every version against when it was published'}>
        {yTicks.map((v) => (
          <g key={v}>
            <line x1={PAD.l} x2={W - PAD.r} y1={y(v)} y2={y(v)} stroke="currentColor" strokeOpacity={0.08} />
            <text x={PAD.l - 6} y={y(v) + 3.5} textAnchor="end" className="fill-muted-foreground text-[10px] tabular-nums">{v}</text>
          </g>
        ))}
        {xTicks.map((t) => (
          <text key={t} x={x(t)} y={H - 8} textAnchor="middle" className="fill-muted-foreground text-[10px] tabular-nums">{xLabel(t)}</text>
        ))}
        {!log && roster.map((r) => <line key={r.bot} x1={PAD.l} x2={W - PAD.r} y1={y(r.elo)} y2={y(r.elo)} stroke="#94a3b8" strokeOpacity={0.35} strokeDasharray="2 4" />)}
        {cut !== null && <line x1={PAD.l} x2={W - PAD.r} y1={y(cut)} y2={y(cut)} stroke="#f87171" strokeOpacity={0.6} strokeDasharray="6 4" />}
        <path d={frontier} fill="none" stroke="#4ade80" strokeWidth={2} strokeLinejoin="round" />
        {labels.map((l) => <text key={l.key} x={W - PAD.r + 6} y={l.y + 3.5} className={`${l.cls} text-[10px]`}>{l.text}</text>)}
        {[...points].sort((a, b) => Number(!!a.frontier) - Number(!!b.frontier)).map((p) => {
          const c = color(p.author)
          return (
            <circle key={p.id} cx={x(p.x)} cy={y(p.elo)} r={p.frontier ? 5.5 : 4.2} className="cursor-pointer" onClick={() => onBot(p.id)}
              fill={p.retired ? 'var(--background)' : c} stroke={p.frontier ? '#4ade80' : p.retired ? c : 'var(--background)'} strokeWidth={p.frontier ? 2 : 1.2}>
              <title>{p.label}</title>
            </circle>
          )
        })}
      </svg>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
        {authors.map((a) => (
          <span key={a} className="flex items-center gap-1.5"><span className="size-2.5 rounded-full" style={{ background: color(a) }} />{a.replace(/^agent:/, '')}</span>
        ))}
        <span className="flex items-center gap-1.5">{log ? <span className="size-2.5 rounded-full bg-slate-400" /> : <span className="h-0 w-4 border-t border-dashed border-slate-400" />}roster CPUs</span>
        <span className="flex items-center gap-1.5"><span className="size-2.5 rounded-full border border-muted-foreground" />retired</span>
        <span className="flex items-center gap-1.5"><span className="h-0.5 w-4 bg-emerald-400" />{log ? 'Pareto front: nothing else is both faster and stronger' : 'best Elo published so far'}</span>
      </div>
    </div>
  )
}
