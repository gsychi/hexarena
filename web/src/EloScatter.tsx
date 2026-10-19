import type { ArenaInfo, League, LeaderboardRow } from './api'

type Kind = 'generation' | 'entrant' | 'challenger'

interface Point {
  key: string
  kind: Kind
  t: number
  elo: number
  label: string
  retired: boolean
  frontier?: boolean
}

const W = 960, H = 340, PAD = { l: 46, r: 150, t: 14, b: 28 }

const COLORS: Record<Kind, string> = { generation: '#fbbf24', entrant: '#a78bfa', challenger: '#64748b' }

function clock(ms: number, spanMs: number) {
  const d = new Date(ms)
  if (spanMs > 36 * 3600_000) return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

/**
 * Every bot's Elo against when it appeared: league generations and arena entrants are
 * rated on the shared standings, rejected league challengers on league games only. The
 * step line is the frontier, the best rating reached by anything published so far.
 */
export function EloScatter({ league, rows, arena, top }: { league: League; rows: LeaderboardRow[]; arena: ArenaInfo | null; top: number }) {
  const rated = new Map(rows.map((r) => [r.bot, r]))
  const points: Point[] = []
  for (const g of league.generations) {
    const r = rated.get(g.id)
    const elo = r?.elo ?? league.elo[g.id] ?? g.elo
    points.push({ key: g.id, kind: 'generation', t: g.created_at, elo, retired: false, label: `${g.name || g.id}\nElo ${Math.round(elo)}${r ? ` over ${r.games} games` : ''}` })
  }
  for (const e of arena?.entrants ?? []) {
    const r = rated.get(e.id)
    if (!r) continue
    points.push({
      key: e.id, kind: 'entrant', t: e.created_at, elo: r.elo, retired: e.retired,
      label: `${e.name} (${e.id}) by ${e.author}\nElo ${Math.round(r.elo)} over ${r.games} games${e.retired ? `\nretired${e.retired_reason ? `: ${e.retired_reason}` : ''}` : ''}${e.note ? `\n${e.note}` : ''}`,
    })
  }
  for (const a of league.attempts) {
    const elo = league.elo[a.id]
    if (a.became !== null || elo === undefined) continue
    points.push({ key: a.id, kind: 'challenger', t: a.at, elo, retired: true, label: `${a.id} (${a.origin}), rejected\nleague Elo ${Math.round(elo)} over ${a.gate.games} games\n${a.changes.join(', ')}` })
  }
  const published = points.filter((p) => p.kind !== 'challenger').sort((a, b) => a.t - b.t)
  if (published.length === 0) {
    return <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No rated bots yet. Ratings appear after their first games.</p>
  }

  const now = Date.now()
  const steps: [number, number][] = []
  let best = -Infinity
  for (const p of published) {
    if (p.elo > best) {
      best = p.elo
      p.frontier = true
      steps.push([p.t, p.elo])
    }
  }
  const placed = new Set(points.map((p) => p.key))
  const roster = rows.filter((r) => !placed.has(r.bot) && !(arena?.entrants ?? []).some((e) => e.id === r.bot))
  const cut = rows.length >= top ? rows[top - 1].elo : null

  const t0 = Math.min(...points.map((p) => p.t)), t1 = Math.max(now, t0 + 60_000)
  const elos = [...points.map((p) => p.elo), ...roster.map((r) => r.elo), ...(cut ? [cut] : [])]
  const lo = Math.floor((Math.min(...elos) - 30) / 50) * 50, hi = Math.ceil((Math.max(...elos) + 30) / 50) * 50
  const x = (t: number) => PAD.l + ((t - t0) / (t1 - t0)) * (W - PAD.l - PAD.r)
  const y = (v: number) => PAD.t + (1 - (v - lo) / (hi - lo)) * (H - PAD.t - PAD.b)
  const yStep = hi - lo > 600 ? 200 : hi - lo > 300 ? 100 : 50
  const yTicks: number[] = []
  for (let v = lo; v <= hi; v += yStep) yTicks.push(v)
  const xTicks = [0, 0.25, 0.5, 0.75, 1].map((f) => t0 + f * (t1 - t0))
  const frontier = steps.map(([t, e], i) => `${i ? `H${x(t).toFixed(1)}V` : `M${x(t).toFixed(1)},`}${y(e).toFixed(1)}`).join('') + `H${x(now).toFixed(1)}`
  const order: Kind[] = ['challenger', 'entrant', 'generation']
  const labels: { key: string; text: string; y: number; cls: string }[] = [
    { key: 'frontier', text: `frontier ${Math.round(best)}`, y: y(best), cls: 'fill-emerald-300 font-medium' },
    ...(cut !== null ? [{ key: 'cut', text: `top ${top} cut ${Math.round(cut)}`, y: y(cut), cls: 'fill-red-300' }] : []),
    ...roster.map((r) => ({ key: r.bot, text: `${r.name} ${Math.round(r.elo)}`, y: y(r.elo), cls: 'fill-muted-foreground' })),
  ].sort((a, b) => a.y - b.y)
  for (let i = 1; i < labels.length; i++) labels[i].y = Math.max(labels[i].y, labels[i - 1].y + 11)

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full" role="img" aria-label="Elo of every bot against the time it appeared">
      {yTicks.map((v) => (
        <g key={v}>
          <line x1={PAD.l} x2={W - PAD.r} y1={y(v)} y2={y(v)} stroke="currentColor" strokeOpacity={0.08} />
          <text x={PAD.l - 6} y={y(v) + 3.5} textAnchor="end" className="fill-muted-foreground text-[10px] tabular-nums">{v}</text>
        </g>
      ))}
      {xTicks.map((t) => (
        <text key={t} x={x(t)} y={H - 8} textAnchor="middle" className="fill-muted-foreground text-[10px] tabular-nums">{clock(t, t1 - t0)}</text>
      ))}
      {roster.map((r) => (
        <g key={r.bot}>
          <line x1={PAD.l} x2={W - PAD.r} y1={y(r.elo)} y2={y(r.elo)} stroke="#94a3b8" strokeOpacity={0.35} strokeDasharray="2 4" />
        </g>
      ))}
      {cut !== null && (
        <g>
          <line x1={PAD.l} x2={W - PAD.r} y1={y(cut)} y2={y(cut)} stroke="#f87171" strokeOpacity={0.6} strokeDasharray="6 4" />
        </g>
      )}
      <path d={frontier} fill="none" stroke="#4ade80" strokeWidth={2} strokeLinejoin="round" />
      {labels.map((l) => (
        <text key={l.key} x={W - PAD.r + 6} y={l.y + 3.5} className={`${l.cls} text-[10px]`}>{l.text}</text>
      ))}
      {order.flatMap((kind) => points.filter((p) => p.kind === kind)).map((p) => {
        const color = COLORS[p.kind]
        const r = p.kind === 'challenger' ? 2.2 : p.frontier ? 5.5 : 4.2
        const hollow = p.kind === 'entrant' && p.retired
        return (
          <circle key={`${p.kind}-${p.key}`} cx={x(p.t)} cy={y(p.elo)} r={r}
            fill={hollow ? 'var(--background)' : color} fillOpacity={p.kind === 'challenger' ? 0.45 : 1}
            stroke={p.frontier ? '#4ade80' : hollow ? color : 'var(--background)'} strokeWidth={p.frontier ? 2 : 1.2}>
            <title>{p.label}</title>
          </circle>
        )
      })}
    </svg>
  )
}
