import { useEffect, useMemo, useState } from 'react'
import { Board } from './Board'
import type { GameEvent, ViewerReplay } from './model'
import { BUILDING_NAMES, PLAYER_COLORS, PLAYER_MARKS, frameCells, hexAt, visibleSet } from './model'

const SPEEDS = [1, 2, 4, 8, 16]

function describe(e: GameEvent, names: string[]): string | null {
  switch (e.type) {
    case 'captured': return `${names[e.player]} took ${e.previous_owner === null ? 'a neutral hex' : `a hex from ${names[e.previous_owner]}`} (${e.troops_left} left)`
    case 'repelled': return `${names[e.attacker]} was repelled (${e.attackers_lost} lost)`
    case 'built': return `${names[e.player]} built a ${e.building}`
    case 'eliminated': return `${names[e.player]} was eliminated`
    case 'moved': return null
    case 'rejected': return `${names[e.player]} action rejected: ${e.reason}`
    default: { const _never: never = e; return _never }
  }
}

export default function App() {
  const [rep, setRep] = useState<ViewerReplay | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)
  const [playing, setPlaying] = useState(false)
  const [speed, setSpeed] = useState(4)
  const [perspective, setPerspective] = useState(-1)
  const [selected, setSelected] = useState<number | null>(null)
  const [drawer, setDrawer] = useState(false)

  useEffect(() => {
    fetch('/sample-replay.json').then((r) => r.json()).then(setRep).catch((e) => setError(String(e)))
  }, [])

  useEffect(() => {
    if (!playing || !rep) return
    const id = setInterval(() => setTick((t) => (t >= rep.frames.length - 1 ? (setPlaying(false), t) : t + 1)), 1000 / speed)
    return () => clearInterval(id)
  }, [playing, speed, rep])

  const cells = useMemo(() => (rep ? frameCells(rep, tick) : new Map()), [rep, tick])
  const visible = useMemo(() => (rep && perspective >= 0 ? visibleSet(rep, cells, perspective) : null), [rep, cells, perspective])

  if (error) return <div className="p-6 text-red-400">Could not load replay: {error}</div>
  if (!rep) return <div className="p-6 text-neutral-400">Loading replay…</div>

  const last = rep.frames.length - 1
  const stats = rep.names.map((_, p) => {
    let land = 0, troops = 0
    for (const c of cells.values()) if (c.owner === p) { land++; troops += c.troops }
    return { land, troops }
  })
  const feed: string[] = []
  for (let t = tick; t >= Math.max(1, tick - 40) && feed.length < 12; t--)
    for (const e of rep.events[t]) {
      const hidden = visible && 'at' in e && !visible.has(hexIdx(rep, e.at))
      const d = hidden ? null : describe(e, rep.names)
      if (d && !(visible && e.type === 'rejected' && e.player !== perspective)) feed.push(`${d} · t${t}`)
    }
  const sel = selected !== null ? cells.get(selected) : undefined
  const selHidden = selected !== null && visible !== null && !visible.has(selected)

  const side = (
    <div className="flex flex-col gap-2 overflow-y-auto">
      <div className="panel p-3">
        <div className="text-[10px] tracking-widest text-neutral-500">REPLAY · {rep.rules_version}</div>
        <div className="mt-1 text-sm text-neutral-100">{rep.names.length}-player free-for-all</div>
        <div className="mt-2 flex flex-wrap gap-1">
          <button className="btn" data-on={perspective < 0} onClick={() => setPerspective(-1)}>God</button>
          {rep.names.map((n, p) => (
            <button key={p} className="btn" data-on={perspective === p} onClick={() => setPerspective(p)} style={{ borderColor: PLAYER_COLORS[p % 8] }}>
              {PLAYER_MARKS[p % 8]} {n.replace('Player ', 'P')}
            </button>
          ))}
        </div>
      </div>
      <div className="panel p-3">
        <table className="w-full text-left">
          <thead className="text-neutral-500"><tr><th></th><th className="text-right">Land</th><th className="text-right">Troops</th><th className="text-right">Final pts</th></tr></thead>
          <tbody>
            {rep.names.map((n, p) => (
              <tr key={p}>
                <td style={{ color: PLAYER_COLORS[p % 8] }}>{PLAYER_MARKS[p % 8]} {n}</td>
                <td className="text-right tabular-nums">{perspective < 0 || perspective === p ? stats[p].land : '?'}</td>
                <td className="text-right tabular-nums">{perspective < 0 || perspective === p ? stats[p].troops : '?'}</td>
                <td className="text-right tabular-nums text-neutral-400">{perspective < 0 && tick === last ? rep.result.points[p] : ''}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="panel p-3">
        <div className="mb-1 text-[10px] tracking-widest text-neutral-500">SELECTION</div>
        {selected === null ? <div className="text-neutral-500">Click a hex.</div> : (() => {
          const h = hexAt(rep.radius, selected), t = rep.terrain[selected]
          return (
            <div>
              <div>({h.q}, {h.r}) · {t?.kind === 'open' ? `open, height ${t.height > 0 ? '+' : ''}${t.height}` : `impassable ${t?.kind}`}</div>
              {selHidden ? <div className="text-neutral-500">Fogged: contents unknown.</div> : t?.kind === 'open' && (
                <div>{sel && sel.owner >= 0 ? rep.names[sel.owner] : 'Neutral'} · {sel ? sel.troops : rep.neutral[selected]} troops{sel && sel.building > 0 ? ` · ${BUILDING_NAMES[sel.building - 1]}` : ''}</div>
              )}
            </div>
          )
        })()}
      </div>
      <div className="panel p-3">
        <div className="mb-1 text-[10px] tracking-widest text-neutral-500">BATTLES & EVENTS</div>
        {feed.length === 0 ? <div className="text-neutral-500">Nothing yet.</div> : feed.map((f, i) => <div key={i} className="py-0.5 text-neutral-300">{f}</div>)}
      </div>
    </div>
  )

  return (
    <div className="flex h-full flex-col gap-2 p-2 lg:flex-row">
      <div className="flex min-h-0 flex-1 flex-col gap-2">
        <div className="min-h-0 flex-1">
          <Board rep={rep} cells={cells} perspective={perspective} visible={visible} selected={selected} onSelect={setSelected} />
        </div>
        <div className="panel flex flex-wrap items-center gap-2 px-3 py-2">
          <button className="btn" onClick={() => setTick(0)}>⏮</button>
          <button className="btn" onClick={() => setTick((t) => Math.max(0, t - 1))}>◀</button>
          <button className="btn" onClick={() => setPlaying((p) => !p)}>{playing ? 'Pause' : 'Play'}</button>
          <button className="btn" onClick={() => setTick((t) => Math.min(last, t + 1))}>▶</button>
          <input type="range" min={0} max={last} value={tick} onChange={(e) => setTick(+e.target.value)} className="min-w-24 flex-1" />
          <span className="tabular-nums text-neutral-400">{tick} / {last}</span>
          <div className="flex gap-1">{SPEEDS.map((s) => <button key={s} className="btn" data-on={speed === s} onClick={() => setSpeed(s)}>{s}/s</button>)}</div>
          <button className="btn lg:hidden" onClick={() => setDrawer(true)}>Stats</button>
        </div>
      </div>
      <aside className="hidden w-80 shrink-0 lg:flex lg:flex-col">{side}</aside>
      {drawer && (
        <div className="fixed inset-x-0 bottom-0 z-10 max-h-[70%] overflow-y-auto rounded-t-xl border-t border-[#252b37] bg-[#08090c] p-2 lg:hidden">
          <button className="btn mb-2" onClick={() => setDrawer(false)}>Close</button>
          {side}
        </div>
      )}
    </div>
  )
}

function hexIdx(rep: ViewerReplay, h: { q: number; r: number }) {
  return (h.r + rep.radius) * (2 * rep.radius + 1) + h.q + rep.radius
}
