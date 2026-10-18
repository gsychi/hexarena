import { useEffect, useMemo, useState } from 'react'
import { Board } from './Board'
import { BuildingIcon, Legend } from './Legend'
import type { GameEvent, ViewerReplay } from './model'
import {
  BUILDING_KEYS, BUILDING_NAMES, HEIGHT_STYLE, PLAYER_COLORS, PLAYER_MARKS, buildPreview, defenseMilli, frameCells, hexAt, hexIndex, visibleSet,
} from './model'

const SPEEDS = [1, 2, 4, 8, 16]

function describe(e: GameEvent, names: string[]): string | null {
  switch (e.type) {
    case 'captured': return `${names[e.player]} took ${e.previous_owner === null ? 'a neutral hex' : `a hex from ${names[e.previous_owner]}`}${e.building ? ` with a ${e.building}` : ''} (${e.troops_left} left)`
    case 'repelled': return `${names[e.attacker]} was repelled (${e.attackers_lost} lost)`
    case 'built': return `${names[e.player]} built a ${e.building}`
    case 'eliminated': return `${names[e.player]} was eliminated`
    case 'surrendered': return `${names[e.capturer]} encircled ${e.hexes.length} hex(es) of ${names[e.from]}`
    case 'gathered': return `${names[e.player]} gathered troops from ${e.sources} hexes`
    case 'links_broken': return 'Links broke after a capture'
    case 'link_changed': return `${names[e.player]} changed a link`
    case 'moved': return null
    case 'rejected': return `${names[e.player]} action rejected: ${e.reason}`
    default: { const _never: never = e; return _never }
  }
}

const eventHex = (e: GameEvent) => ('at' in e ? e.at : null)

export default function App() {
  const [rep, setRep] = useState<ViewerReplay | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)
  const [playing, setPlaying] = useState(false)
  const [speed, setSpeed] = useState(4)
  const [perspective, setPerspective] = useState(-1)
  const [selected, setSelected] = useState<number | null>(null)
  const [drawer, setDrawer] = useState(false)
  const [heightMode, setHeightMode] = useState(false)
  const [tab, setTab] = useState<'match' | 'guide' | 'report'>('match')

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
      const at = eventHex(e)
      const hidden = visible && at && !visible.has(hexIndex(rep.radius, at))
      const d = hidden ? null : describe(e, rep.names)
      if (d && !(visible && e.type === 'rejected' && e.player !== perspective)) feed.push(`${d} · t${t}`)
    }

  const selHidden = selected !== null && visible !== null && !visible.has(selected)
  const selection = (() => {
    if (selected === null) return <div className="text-neutral-500">Click a hex.</div>
    const h = hexAt(rep.radius, selected), t = rep.terrain[selected]
    if (!t) return null
    if (t.kind !== 'open') return <div>({h.q}, {h.r}) · impassable {t.kind === 'water' ? 'water' : 'rock'}. Cannot be entered, owned, or built on.</div>
    const hs = HEIGHT_STYLE[t.height]
    const c = cells.get(selected)
    const wallLinks = c && c.building === 4 ? c.links.filter((l: number) => l >= 0).length : null
    return (
      <div className="space-y-1.5">
        <div>({h.q}, {h.r}) · <span style={{ color: hs.tag }} className="font-bold">{hs.label}</span> {hs.name}</div>
        {selHidden ? <div className="text-neutral-500">Fogged: contents unknown.</div> : (
          <>
            <div>{c && c.owner >= 0 ? <span style={{ color: PLAYER_COLORS[c.owner % 8] }}>{PLAYER_MARKS[c.owner % 8]} {rep.names[c.owner]}</span> : 'Neutral'} · {c ? c.troops : rep.neutral[selected]} troops</div>
            {c && c.building > 0 && <BuildingDetail rep={rep} c={c} wallLinks={wallLinks} />}
            <div className="text-[10px] tracking-widest text-neutral-500">DEFENSE IF ATTACKED FROM HEIGHT</div>
            <div className="grid grid-cols-5 gap-1 text-center">
              {[-2, -1, 0, 1, 2].map((a) => {
                const d = defenseMilli(rep.rules, a, t.height, wallLinks)
                const capped = d >= rep.rules.maximum_total_defense_milli
                return (
                  <div key={a} className="rounded border border-[#1c2029] py-0.5">
                    <div style={{ color: HEIGHT_STYLE[a].tag }}>{HEIGHT_STYLE[a].label}</div>
                    <div className="tabular-nums text-neutral-100">{(d / 1000).toFixed(2)}x</div>
                    {capped && <div className="text-[9px] text-amber-300">cap</div>}
                  </div>
                )
              })}
            </div>
            {c && c.owner >= 0 && c.building === 0 && (
              <>
                <div className="pt-1 text-[10px] tracking-widest text-neutral-500">BUILD HERE (costs paid from this hex's troops, 1 must remain)</div>
                {BUILDING_KEYS.map((_, i) => {
                  const p = buildPreview(rep, cells, selected, i + 1)
                  return (
                    <div key={i} className={`flex items-center gap-2 ${p.legal ? '' : 'opacity-50'}`}>
                      <BuildingIcon kind={i + 1} size={22} />
                      <div className="flex-1">
                        <span className="font-semibold">{BUILDING_NAMES[i]}</span>{' '}
                        {p.legal ? (
                          <span>
                            {p.discounted && <span className="text-neutral-500 line-through">{p.baseTroops}</span>} <b>{p.troops}</b> troops
                            {p.baseResource > 0 || p.resource > 0 ? <> · {p.discounted && p.baseResource !== p.resource && <span className="text-neutral-500 line-through">{p.baseResource}</span>} <b>{p.resource}</b> Resource</> : ' · 0 Resource'}
                            {p.discounted && <span className="ml-1 text-cyan-300">Quarry discount −{p.baseTroops - p.troops} troops{p.baseResource !== p.resource ? `, −${p.baseResource - p.resource} Resource` : ''}</span>}
                          </span>
                        ) : <span className="text-neutral-400">{p.reason}</span>}
                      </div>
                    </div>
                  )
                })}
              </>
            )}
          </>
        )}
      </div>
    )
  })()

  const side = (
    <div className="flex flex-col gap-2 overflow-y-auto">
      <div className="panel p-3">
        <div className="text-[10px] tracking-widest text-neutral-500">REPLAY · rules {rep.rules.version}</div>
        <div className="mt-1 text-sm text-neutral-100">{rep.names.length}-player free-for-all</div>
        <div className="mt-2 flex flex-wrap gap-1">
          <button className="btn" data-on={perspective < 0} onClick={() => setPerspective(-1)}>God</button>
          {rep.names.map((n, p) => (
            <button key={p} className="btn" data-on={perspective === p} onClick={() => setPerspective(p)} style={{ borderColor: PLAYER_COLORS[p % 8] }}>
              {PLAYER_MARKS[p % 8]} {n.replace('Player ', 'P')}
            </button>
          ))}
        </div>
        <div className="mt-2 flex gap-1">
          {(['match', 'guide', 'report'] as const).map((k) => <button key={k} className="btn capitalize" data-on={tab === k} onClick={() => setTab(k)}>{k === 'guide' ? 'Legend' : k === 'report' ? 'Building stats' : 'Match'}</button>)}
          <button className="btn ml-auto" data-on={heightMode} onClick={() => setHeightMode((v) => !v)}>Heights</button>
        </div>
      </div>
      {tab === 'guide' && <Legend />}
      {tab === 'report' && <Report rep={rep} />}
      {tab === 'match' && (
        <>
          <div className="panel p-3">
            <table className="w-full text-left">
              <thead className="text-neutral-500"><tr><th></th><th className="text-right">Land</th><th className="text-right">Troops</th><th className="text-right">Final pts</th></tr></thead>
              <tbody>
                {rep.names.map((n, p) => (
                  <tr key={p}>
                    <td style={{ color: PLAYER_COLORS[p % 8] }}>{PLAYER_MARKS[p % 8]} {n}</td>
                    <td className="text-right tabular-nums">{perspective < 0 || perspective === p ? stats[p].land : '?'}</td>
                    <td className="text-right tabular-nums">{perspective < 0 || perspective === p ? stats[p].troops : '?'}</td>
                    <td className="text-right tabular-nums text-neutral-400">{perspective < 0 && tick === last ? (rep.result.placements.find((x) => x.player === p)!.points / 1000).toFixed(1) : ''}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="panel p-3">
            <div className="mb-1 text-[10px] tracking-widest text-neutral-500">SELECTION</div>
            {selection}
          </div>
          <div className="panel p-3">
            <div className="mb-1 text-[10px] tracking-widest text-neutral-500">BATTLES & EVENTS</div>
            {feed.length === 0 ? <div className="text-neutral-500">Nothing yet.</div> : feed.map((f, i) => <div key={i} className="py-0.5 text-neutral-300">{f}</div>)}
          </div>
        </>
      )}
    </div>
  )

  return (
    <div className="flex h-full flex-col gap-2 p-2 lg:flex-row">
      <div className="flex min-h-0 flex-1 flex-col gap-2">
        <div className="min-h-0 flex-1">
          <Board rep={rep} cells={cells} perspective={perspective} visible={visible} selected={selected} heightMode={heightMode} onSelect={setSelected} />
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
      <aside className="hidden w-96 shrink-0 lg:flex lg:flex-col">{side}</aside>
      {drawer && (
        <div className="fixed inset-x-0 bottom-0 z-10 max-h-[70%] overflow-y-auto rounded-t-xl border-t border-[#252b37] bg-[#08090c] p-2 lg:hidden">
          <button className="btn mb-2" onClick={() => setDrawer(false)}>Close</button>
          {side}
        </div>
      )}
    </div>
  )
}

function BuildingDetail({ rep, c, wallLinks }: { rep: ViewerReplay; c: ReturnType<typeof frameCells> extends Map<number, infer V> ? V : never; wallLinks: number | null }) {
  const r = rep.rules
  const linked = c.links.filter((l) => l >= 0).length
  const cd = c.cooldown > 0 ? ` · relink in ${c.cooldown} ticks` : ' · relink ready'
  return (
    <div className="flex items-start gap-2 rounded border border-[#1c2029] p-2">
      <BuildingIcon kind={c.building} size={30} dashed={(c.building === 2 && c.links[0] < 0) || wallLinks === 0} double={(c.building === 2 && c.links[0] >= 0) || wallLinks === 2} />
      <div>
        <div className="font-semibold">{BUILDING_NAMES[c.building - 1]}</div>
        {c.building === 1 && <div>{linked}/{r.farm.sockets} support sockets used{cd}. +1 Resource in {c.remaining} ticks.</div>}
        {c.building === 2 && <div>{c.links[0] >= 0 ? `Produces ${r.base_income + r.village.bonus + r.village.farm_bonus}/tick: Farm-supported.` : `Produces ${r.base_income + r.village.bonus}/tick: unsupported (a Farm link gives +${r.village.farm_bonus}).`}</div>}
        {c.building === 3 && <div>+1 Resource in {c.remaining} ticks. Adjacent builds: −{r.quarry.troop_discount_pct}% troops, −{r.quarry.resource_discount} Resource.</div>}
        {c.building === 4 && <div>{wallLinks}/{r.wall.max_links} links · wall {(r.wall.defense_milli_by_links[wallLinks ?? 0] / 1000).toFixed(1)}x{cd}.</div>}
      </div>
    </div>
  )
}

function Report({ rep }: { rep: ViewerReplay }) {
  const rows = BUILDING_KEYS.map((k, i) => ({ k, name: BUILDING_NAMES[i], r: rep.building_report[k] }))
  return (
    <div className="panel p-3">
      <div className="mb-1 text-[10px] tracking-widest text-neutral-500">BUILDING INSTRUMENTATION (THIS MATCH)</div>
      <table className="w-full text-left text-[11px]">
        <thead className="text-neutral-500"><tr><th></th>{rows.map((r) => <th key={r.k} className="text-right">{r.name}</th>)}</tr></thead>
        <tbody>
          {([
            ['Built', (r) => String(r.constructed)],
            ['Avg tick built', (r) => r.avg_construction_tick.toFixed(0)],
            ['Avg troops paid', (r) => r.avg_troops_paid.toFixed(0)],
            ['Captured share', (r) => `${(r.captured_share * 100).toFixed(0)}%`],
            ['Resource made', (r) => r.avg_resource_generated.toFixed(1)],
            ['Village bonus troops', (r) => r.avg_village_bonus_troops.toFixed(0)],
            ['Linked time', (r) => `${(r.linked_share * 100).toFixed(0)}%`],
            ['Avg wall x', (r) => r.avg_wall_multiplier.toFixed(2)],
            ['Wall fights', (r) => r.avg_wall_fights.toFixed(1)],
            ['Repaid share', (r) => `${(r.repaid_share * 100).toFixed(0)}%`],
            ['Builder avg place', (r) => r.avg_builder_place.toFixed(2)],
          ] as [string, (r: ViewerReplay['building_report']['farm']) => string][]).map(([label, f]) => (
            <tr key={label}><td className="text-neutral-400">{label}</td>{rows.map((r) => <td key={r.k} className="text-right tabular-nums">{r.r.constructed ? f(r.r) : '–'}</td>)}</tr>
          ))}
        </tbody>
      </table>
      <div className="mt-2 text-[10px] text-neutral-500">Aggregate reports across many self-play matches: <code>hexarena report --matches 50</code>. Values are provisional evidence, not balance claims.</div>
    </div>
  )
}
