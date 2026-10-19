import { ArrowLeft, ChevronFirst, ChevronLast, Link2, Loader2, Mountain, Pause, Play, RotateCcw, SkipBack, SkipForward, Swords, Trophy } from 'lucide-react'
import { memo, useEffect, useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Slider } from '@/components/ui/slider'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api } from './api'
import { Board, buildingPips } from './Board'
import { BuildingIcon, Legend } from './Legend'
import type { Cell, GameEvent, Placement, ViewerReplay } from './model'
import {
  BUILDING_KEYS, BUILDING_NAMES, HEIGHT_STYLE, PLAYER_COLORS, PLAYER_MARKS, adjacentQuarries, buildPreview, defenseMilli, frameCells, hexAt, hexIndex, loadViewer, usesResource, visibleSet,
} from './model'

const SPEEDS = [1, 2, 4, 8, 16]

interface FeedItem { color: string; text: string; tick: number; kind: number }

const kindOf = (name: string) => BUILDING_KEYS.indexOf(name as typeof BUILDING_KEYS[number]) + 1

function describe(e: GameEvent, names: string[]): { player: number; text: string; kind?: number } | null {
  switch (e.type) {
    case 'captured': return { player: e.player, text: `${names[e.player]} took ${e.previous_owner === null ? 'a neutral hex' : `a hex from ${names[e.previous_owner]}`}${e.building ? ` with its ${e.building}` : ''} (${e.troops_left} left)` }
    case 'repelled': return { player: e.attacker, text: `${names[e.attacker]} was repelled (${e.attackers_lost} lost)` }
    case 'built': return { player: e.player, text: `${names[e.player]} built a ${BUILDING_NAMES[kindOf(e.building) - 1]}`, kind: kindOf(e.building) }
    case 'eliminated': return { player: e.player, text: `${names[e.player]} was eliminated` }
    case 'surrendered': return { player: e.capturer, text: `${names[e.capturer]} encircled ${e.hexes.length} hex(es) of ${names[e.from]}` }
    case 'gathered': return { player: e.player, text: `${names[e.player]} gathered troops from ${e.sources} hexes` }
    case 'links_broken': return null
    case 'link_changed': return { player: e.player, text: `${names[e.player]} changed a link` }
    case 'moved': return null
    case 'rejected': return null
    default: { const _never: never = e; return _never }
  }
}

const eventHex = (e: GameEvent) => ('at' in e ? e.at : null)

/** One seat's Elo going into the game and, once the game is rated, after it. */
interface SeatElo { before: number; after: number | null }

/** `1532`, plus the change once the game is rated (`+12` / `−8`). */
function EloTag({ elo, change, block }: { elo?: SeatElo | null; change: boolean; block?: boolean }) {
  if (!elo) return null
  const d = elo.after === null ? null : Math.round(elo.after - elo.before)
  return (
    <span className={`${block ? 'mt-0.5 block pl-6' : 'ml-1.5'} whitespace-nowrap text-[11px] font-normal tabular-nums text-muted-foreground`} title={elo.after === null ? 'Elo going into this game' : `Elo ${Math.round(elo.before)} before this game, ${Math.round(elo.after)} after`}>
      ({Math.round(elo.before)})
      {change && d !== null && (
        <span className={`ml-1 font-medium ${d > 0 ? 'text-emerald-400' : d < 0 ? 'text-rose-400' : 'text-muted-foreground'}`}>{d > 0 ? `+${d}` : d < 0 ? `−${-d}` : '±0'}</span>
      )}
    </span>
  )
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return <div className="text-[10px] font-medium uppercase tracking-[0.14em] text-muted-foreground">{children}</div>
}

export function Viewer({ src, match, label, onBack }: { src: string; match?: string; label: string; onBack: () => void }) {
  const [rep, setRep] = useState<ViewerReplay | null>(null)
  const [elo, setElo] = useState<(SeatElo | null)[]>([])
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)
  const [playing, setPlaying] = useState(false)
  const [speed, setSpeed] = useState(4)
  const [perspective, setPerspective] = useState(-1)
  const [selected, setSelected] = useState<number | null>(null)
  const [drawer, setDrawer] = useState(false)
  const [heightMode, setHeightMode] = useState(false)
  const [tab, setTab] = useState('events')
  const [hideEnd, setHideEnd] = useState(false)

  useEffect(() => {
    setRep(null); setError(null); setTick(0); setPlaying(false); setSelected(null); setElo([])
    // Seats and ratings are extras: the replay still plays without them.
    const extras = match
      ? Promise.all([api.match(match), api.matchElo(match)]).catch(() => null)
      : Promise.resolve(null)
    Promise.all([fetch(src).then((r) => { if (!r.ok) throw new Error(`${r.status} ${r.statusText}`); return r.json() }), extras])
      .then(([raw, extra]) => {
        const loaded = loadViewer(raw)
        if (extra) {
          const [summary, ratings] = extra
          const byBot = new Map(ratings.bots.map((b) => [b.bot, b]))
          const seats = summary.seats
          const once = (bot: string) => seats.filter((x) => x.bot === bot).length === 1
          if (seats.length === loaded.names.length) {
            loaded.names = loaded.names.map((n, p) => (once(seats[p].bot) ? byBot.get(seats[p].bot)?.name ?? n : n))
            setElo(seats.map((x) => {
              const b = byBot.get(x.bot)
              return b && b.before !== null ? { before: b.before, after: b.after } : null
            }))
          }
        }
        const want = new URLSearchParams(location.search).get('tick')
        const lastTick = loaded.store.last
        if (want) setTick(want === 'end' ? lastTick : Math.min(lastTick, Math.max(0, Number(want) || 0)))
        setRep(loaded)
      })
      .catch((e) => setError(String(e)))
  }, [src, match])

  useEffect(() => {
    if (!playing || !rep) return
    const id = setInterval(() => setTick((t) => (t >= rep.store.last ? (setPlaying(false), t) : t + 1)), 1000 / speed)
    return () => clearInterval(id)
  }, [playing, speed, rep])

  useEffect(() => { if (rep && tick < rep.store.last) setHideEnd(false) }, [tick, rep])

  const cells = useMemo(() => (rep ? frameCells(rep, tick) : new Map<number, Cell>()), [rep, tick])
  const visible = useMemo(() => (rep && perspective >= 0 ? visibleSet(rep, cells, perspective) : null), [rep, cells, perspective])
  const builtBy = useMemo(() => {
    if (!rep) return []
    const acc = [0, 0, 0, 0]
    return rep.events.map((evs) => {
      for (const e of evs) if (e.type === 'built') acc[kindOf(e.building) - 1]++
      return acc.slice()
    })
  }, [rep])

  if (error) return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
      <div className="text-destructive">Could not load this replay: {error}</div>
      <Button variant="outline" onClick={onBack}><ArrowLeft />Back to games</Button>
    </div>
  )
  if (!rep) return <div className="flex h-full items-center justify-center gap-2 text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading replay…</div>

  const last = rep.store.last
  const r = rep.rules
  const stats = rep.names.map((_, p) => {
    let land = 0, troops = 0, income = 0
    const built = [0, 0, 0, 0]
    for (const [idx, c] of cells) {
      if (c.owner !== p) continue
      land++; troops += c.troops; income += r.base_income
      if (c.building > 0) built[c.building - 1]++
      if (c.building === 2) income += r.village.bonus + c.links.filter((l) => l >= 0).length * r.village.farm_bonus + adjacentQuarries(rep, cells, idx) * (r.quarry.village_bonus ?? 0)
    }
    return { land, troops, income, built }
  })
  const builtSoFar = builtBy[tick] ?? [0, 0, 0, 0]
  const feed: FeedItem[] = []
  for (let t = tick; t >= Math.max(1, tick - 60) && feed.length < 14; t--)
    for (const e of rep.events[t]) {
      const at = eventHex(e)
      if (visible && at && !visible.has(hexIndex(rep.radius, at))) continue
      const d = describe(e, rep.names)
      if (d) feed.push({ color: PLAYER_COLORS[d.player % 8], text: d.text, tick: t, kind: d.kind ?? 0 })
    }
  const knows = (p: number) => perspective < 0 || perspective === p
  const final = tick === last && perspective < 0

  const sidebar = (
    <div className="flex flex-col gap-3">
      <div className="px-1">
        <Button variant="ghost" size="sm" className="-ml-2 mb-1" onClick={onBack}><ArrowLeft />All games</Button>
        <SectionLabel>{label} · rules {r.version}</SectionLabel>
        <div className="mt-1 truncate text-sm font-medium">{rep.names.join(' vs ')}</div>
        <div className="text-xs text-muted-foreground">{rep.names.length}-player free-for-all · radius {rep.radius}</div>
      </div>

      <div className="flex flex-wrap gap-1 rounded-lg bg-muted p-1">
        <Button size="xs" variant={perspective < 0 ? 'default' : 'ghost'} onClick={() => setPerspective(-1)}>God view</Button>
        {rep.names.map((n, p) => (
          <Button key={p} size="xs" variant={perspective === p ? 'default' : 'ghost'} onClick={() => setPerspective(p)}>
            <span className="size-2 rounded-full" style={{ background: PLAYER_COLORS[p % 8] }} />{n.replace('Player ', 'P')}
          </Button>
        ))}
      </div>

      <Card size="sm" className="gap-0 py-2">
        <CardContent className="px-2">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="h-7" />
                <TableHead className="h-7 text-right">Land</TableHead>
                <TableHead className="h-7 text-right">Troops</TableHead>
                <TableHead className="h-7 text-right">Per tick</TableHead>
                {final && <TableHead className="h-7 text-right">Pts</TableHead>}
              </TableRow>
            </TableHeader>
            <TableBody>
              {rep.names.map((n, p) => (
                <TableRow key={p} className="hover:bg-transparent">
                  <TableCell className="py-1.5 font-medium">
                    <span className="mr-2 inline-block size-2 rounded-full" style={{ background: PLAYER_COLORS[p % 8] }} />
                    <span className="mr-1 text-[10px]" style={{ color: PLAYER_COLORS[p % 8] }}>{PLAYER_MARKS[p % 8]}</span>
                    <span className="inline-block max-w-40 truncate align-bottom">{n}</span>
                    <EloTag elo={elo[p]} change={tick === last} block />
                  </TableCell>
                  <TableCell className="py-1.5 text-right tabular-nums">{knows(p) ? stats[p].land : '—'}</TableCell>
                  <TableCell className="py-1.5 text-right tabular-nums">{knows(p) ? stats[p].troops : '—'}</TableCell>
                  <TableCell className="py-1.5 text-right tabular-nums">{knows(p) ? `+${stats[p].income}` : '—'}</TableCell>
                  {final && <TableCell className="py-1.5 text-right tabular-nums">{(rep.result.placements.find((x) => x.player === p)!.points / 1000).toFixed(1)}</TableCell>}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Card size="sm" className="gap-2 py-3">
        <CardHeader className="flex items-center justify-between">
          <CardTitle className="text-sm">Buildings standing</CardTitle>
          {perspective < 0 && <span className="text-[11px] text-muted-foreground">{builtSoFar.reduce((a, b) => a + b, 0)} built so far</span>}
        </CardHeader>
        <CardContent className="space-y-1.5">
          <div className="grid grid-cols-[1fr_repeat(4,2.75rem)] items-center gap-1 text-[11px] text-muted-foreground">
            <span />
            {BUILDING_NAMES.map((n, i) => <span key={n} className="flex justify-center" title={n}><BuildingIcon kind={i + 1} size={20} /></span>)}
          </div>
          {rep.names.map((n, p) => (
            <div key={p} className="grid grid-cols-[1fr_repeat(4,2.75rem)] items-center gap-1 text-xs">
              <span className="truncate"><span className="mr-2 inline-block size-2 rounded-full" style={{ background: PLAYER_COLORS[p % 8] }} />{n}</span>
              {stats[p].built.map((c, i) => (
                <span key={i} className={`text-center tabular-nums ${knows(p) && c > 0 ? 'font-medium text-foreground' : 'text-muted-foreground'}`}>{knows(p) ? c : '—'}</span>
              ))}
            </div>
          ))}
        </CardContent>
      </Card>

      <Tabs value={tab} onValueChange={(v) => setTab(String(v))}>
        <TabsList className="w-full">
          <TabsTrigger value="events"><Swords />Battles</TabsTrigger>
          <TabsTrigger value="hex"><Mountain />Hex</TabsTrigger>
          <TabsTrigger value="legend">Legend</TabsTrigger>
          <TabsTrigger value="stats">Buildings</TabsTrigger>
        </TabsList>
        <TabsContent value="events">
          <Card size="sm">
            <CardHeader><CardTitle>Battles</CardTitle></CardHeader>
            <CardContent>
              {feed.length === 0 ? <div className="text-xs text-muted-foreground">No visible fighting yet. Press play.</div> : (
                <ul className="space-y-2">
                  {feed.map((f, i) => (
                    <li key={i} className={`flex gap-2 text-xs leading-snug ${f.kind ? 'items-center rounded-md bg-muted/40 py-0.5 pr-1' : ''}`}>
                      {f.kind ? <BuildingIcon kind={f.kind} size={18} /> : <span className="mt-1 size-2 shrink-0 rounded-full" style={{ background: f.color }} />}
                      <span>{f.kind ? <span style={{ color: f.color }}>{f.text}</span> : f.text} <span className="text-muted-foreground">· t{f.tick}</span></span>
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="hex"><HexPanel rep={rep} cells={cells} selected={selected} visible={visible} /></TabsContent>
        <TabsContent value="legend"><StaticLegend /></TabsContent>
        <TabsContent value="stats"><MatchReport rep={rep} /></TabsContent>
      </Tabs>
    </div>
  )

  return (
    <div className="flex h-full flex-col gap-3 bg-background p-3 text-foreground lg:flex-row">
      <div className="flex min-h-0 flex-1 flex-col gap-3">
        <div className="relative min-h-0 flex-1">
          <Board rep={rep} cells={cells} perspective={perspective} visible={visible} selected={selected} heightMode={heightMode} tick={tick}
            onSelect={(i) => { setSelected(i); if (i !== null) setTab('hex') }} />
          {tick === last && !hideEnd && <MatchOver rep={rep} elo={elo} onReplay={() => { setTick(0); setPlaying(true) }} onClose={() => setHideEnd(true)} />}
        </div>
        <Card size="sm" className="py-2">
          <CardContent className="flex flex-wrap items-center gap-2">
            <Button variant="ghost" size="icon-sm" onClick={() => setTick(0)}><ChevronFirst /></Button>
            <Button variant="ghost" size="icon-sm" onClick={() => setTick((t) => Math.max(0, t - 1))}><SkipBack /></Button>
            <Button variant="secondary" size="icon-sm" onClick={() => setPlaying((p) => !p)}>{playing ? <Pause /> : <Play />}</Button>
            <Button variant="ghost" size="icon-sm" onClick={() => setTick((t) => Math.min(last, t + 1))}><SkipForward /></Button>
            <Button variant="ghost" size="icon-sm" onClick={() => setTick(last)}><ChevronLast /></Button>
            <Slider className="mx-2 min-w-32 flex-1" min={0} max={last} value={[tick]} onValueChange={(v) => setTick(Array.isArray(v) ? v[0] : v)} />
            <span className="w-20 text-right text-xs tabular-nums text-muted-foreground">{tick} / {last}</span>
            <ToggleGroup variant="outline" size="sm" value={[String(speed)]} onValueChange={(v) => v.length && setSpeed(Number(v[v.length - 1]))}>
              {SPEEDS.map((s) => <ToggleGroupItem key={s} value={String(s)} className="px-2 text-xs data-pressed:bg-foreground data-pressed:text-background">{s}/s</ToggleGroupItem>)}
            </ToggleGroup>
            <Button variant={heightMode ? 'default' : 'outline'} size="sm" onClick={() => setHeightMode((v) => !v)}><Mountain />Heights</Button>
            <Button variant="outline" size="sm" className="lg:hidden" onClick={() => setDrawer(true)}>Details</Button>
          </CardContent>
        </Card>
      </div>
      <aside className="hidden w-[360px] shrink-0 lg:block">
        <ScrollArea className="h-full pr-2">{sidebar}</ScrollArea>
      </aside>
      <Sheet open={drawer} onOpenChange={setDrawer}>
        <SheetContent side="bottom" className="max-h-[75vh] overflow-y-auto">
          <SheetHeader><SheetTitle>Match details</SheetTitle></SheetHeader>
          <div className="px-4 pb-4">{sidebar}</div>
        </SheetContent>
      </Sheet>
    </div>
  )
}

function MatchOver({ rep, elo, onReplay, onClose }: { rep: ViewerReplay; elo: (SeatElo | null)[]; onReplay: () => void; onClose: () => void }) {
  const ps = [...rep.result.placements].sort((a, b) => a.place_start - b.place_start || a.player - b.player)
  const survivors = ps.filter((p) => p.land > 0).length
  const winner = ps[0]
  const passable = rep.terrain.filter((t) => t?.kind === 'open').length
  const tick = rep.result.final_tick
  const reason =
    winner.land === passable
      ? `Conquered all ${passable} hexes at tick ${tick}.`
      : survivors <= 1 && !rep.rules.end_on_total_conquest
        ? `Last player standing at tick ${tick}.`
        : `Cut off at tick ${tick} before anyone took all the land. Survivors ranked by land, then troops.`
  const place = (p: Placement) => (p.place_start === p.place_end ? `#${p.place_start}` : `#${p.place_start}–${p.place_end}`)
  return (
    <div className="absolute inset-0 flex items-center justify-center bg-background/55 p-4 backdrop-blur-[2px]">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <SectionLabel>Match over</SectionLabel>
          <CardTitle className="flex items-center gap-2 text-lg">
            <Trophy className="size-5 text-amber-300" />
            <span style={{ color: PLAYER_COLORS[winner.player % 8] }}>{rep.names[winner.player]}</span> wins
          </CardTitle>
          <div className="text-xs text-muted-foreground">{reason}</div>
        </CardHeader>
        <CardContent className="space-y-3">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="h-7">Place</TableHead><TableHead className="h-7" />
                <TableHead className="h-7 text-right">Land</TableHead><TableHead className="h-7 text-right">Points</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {ps.map((p) => (
                <TableRow key={p.player} className="hover:bg-transparent">
                  <TableCell className="py-1.5 tabular-nums">{place(p)}</TableCell>
                  <TableCell className="py-1.5 font-medium">
                    <span className="mr-2 inline-block size-2 rounded-full" style={{ background: PLAYER_COLORS[p.player % 8] }} />{rep.names[p.player]}
                    <EloTag elo={elo[p.player]} change />
                    {p.land === 0 && <span className="ml-1.5 text-[10px] text-muted-foreground">eliminated</span>}
                  </TableCell>
                  <TableCell className="py-1.5 text-right tabular-nums">{p.land}</TableCell>
                  <TableCell className="py-1.5 text-right tabular-nums">{(p.points / 1000).toFixed(1)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <div className="flex gap-2">
            <Button className="flex-1" onClick={onReplay}><RotateCcw />Watch again</Button>
            <Button variant="outline" className="flex-1" onClick={onClose}>View final board</Button>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}

function HexPanel({ rep, cells, selected, visible }: { rep: ViewerReplay; cells: Map<number, Cell>; selected: number | null; visible: Set<number> | null }) {
  if (selected === null) return <Card size="sm"><CardContent className="text-xs text-muted-foreground">Click any hex to see its terrain, defense, buildings, and build costs.</CardContent></Card>
  const h = hexAt(rep.radius, selected), t = rep.terrain[selected]
  if (!t) return null
  if (t.kind !== 'open') return <Card size="sm"><CardContent className="text-xs">({h.q}, {h.r}) · impassable {t.kind === 'water' ? 'water' : 'rock'}. Cannot be entered, owned, or built on.</CardContent></Card>
  const hs = HEIGHT_STYLE[t.height]
  const hidden = visible !== null && !visible.has(selected)
  const c = cells.get(selected)
  const wallLinks = c && c.building === 4 ? c.links.filter((l) => l >= 0).length : null
  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <span className="inline-block size-3 rounded-sm" style={{ background: hs.fill }} />
          {hs.name} <Badge variant="outline">{hs.label}</Badge>
          <span className="ml-auto text-xs font-normal text-muted-foreground">({h.q}, {h.r})</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-xs">
        {hidden ? <div className="text-muted-foreground">Fogged: ownership, troops, and buildings are unknown.</div> : (
          <>
            <div>
              {c && c.owner >= 0 ? <span style={{ color: PLAYER_COLORS[c.owner % 8] }}>{rep.names[c.owner]}</span> : 'Neutral'} · {c ? c.troops : rep.neutral[selected]} troops
            </div>
            {c && c.building > 0 && <BuildingDetail rep={rep} c={c} wallLinks={wallLinks} quarries={adjacentQuarries(rep, cells, selected)} />}
            <div className="space-y-1.5">
              <SectionLabel>Defense if attacked from height</SectionLabel>
              <div className="grid grid-cols-5 gap-1 text-center">
                {[-2, -1, 0, 1, 2].map((a) => {
                  const d = defenseMilli(rep.rules, a, t.height, wallLinks)
                  return (
                    <div key={a} className="rounded-md border py-1">
                      <div className="text-[10px] text-muted-foreground">{HEIGHT_STYLE[a].label}</div>
                      <div className="font-medium tabular-nums">{(d / 1000).toFixed(2)}×</div>
                      {d >= rep.rules.maximum_total_defense_milli && <div className="text-[9px] text-amber-300">cap</div>}
                    </div>
                  )
                })}
              </div>
            </div>
            {c && c.owner >= 0 && c.building === 0 && (
              <div className="space-y-1.5">
                <SectionLabel>Build here · paid from this hex, 1 troop stays</SectionLabel>
                {BUILDING_KEYS.map((_, i) => {
                  const p = buildPreview(rep, cells, selected, i + 1)
                  return (
                    <div key={i} className={`flex items-center gap-2 rounded-md border px-2 py-1.5 ${p.legal ? '' : 'opacity-45'}`}>
                      <BuildingIcon kind={i + 1} size={22} />
                      <div className="flex-1">
                        <div className="font-medium">{BUILDING_NAMES[i]}</div>
                        {p.legal ? (
                          <div className="text-muted-foreground">
                            {p.discounted && <span className="mr-1 line-through">{p.baseTroops}</span>}<span className="text-foreground">{p.troops}</span> troops
                            {usesResource(rep.rules) && <>{' · '}{p.discounted && p.baseResource !== p.resource && <span className="mr-1 line-through">{p.baseResource}</span>}<span className="text-foreground">{p.resource}</span> Resource</>}
                            {p.discounted && <span className="ml-1 text-cyan-300">Quarry −{p.baseTroops - p.troops}{p.baseResource !== p.resource ? `, −${p.baseResource - p.resource} Res` : ''}</span>}
                          </div>
                        ) : <div className="text-muted-foreground">{p.reason}</div>}
                      </div>
                    </div>
                  )
                })}
              </div>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}

function BuildingDetail({ rep, c, wallLinks, quarries }: { rep: ViewerReplay; c: Cell; wallLinks: number | null; quarries: number }) {
  const r = rep.rules
  const qb = r.quarry.village_bonus ?? 0
  const linked = c.links.filter((l) => l >= 0).length
  const cd = c.cooldown > 0 ? `relink in ${c.cooldown} ticks` : 'relink ready'
  return (
    <div className="flex items-start gap-3 rounded-lg border bg-muted/30 p-2.5">
      <BuildingIcon kind={c.building} size={36} pips={buildingPips(c, r)} />
      <div className="space-y-0.5">
        <div className="font-medium">{BUILDING_NAMES[c.building - 1]}</div>
        {c.building === 1 && (r.farm.adjacent_boost
          ? <div className="text-muted-foreground"><Link2 className="mr-1 inline size-3" />Boosting {linked} adjacent Village{linked === 1 ? '' : 's'} by +{r.village.farm_bonus}/tick each</div>
          : <div className="text-muted-foreground"><Link2 className="mr-1 inline size-3" />{linked}/{r.farm.sockets} Villages supported, +{r.village.farm_bonus}/tick each · {cd}{usesResource(r) && <><br />+1 Resource in {c.remaining} ticks</>}</div>)}
        {c.building === 2 && (r.farm.adjacent_boost
          ? <div className="text-muted-foreground"><span className="text-foreground">{r.base_income + r.village.bonus + linked * r.village.farm_bonus + quarries * qb}/tick</span> · {linked} adjacent Farm{linked === 1 ? '' : 's'}, each adds +{r.village.farm_bonus}{qb > 0 && <> · {quarries} adjacent Quarr{quarries === 1 ? 'y' : 'ies'}, each adds +{qb}</>}</div>
          : <div className="text-muted-foreground">{c.links[0] >= 0 ? <><span className="text-foreground">{r.base_income + r.village.bonus + r.village.farm_bonus}/tick</span> · supported by a Farm</> : <><span className="text-foreground">{r.base_income + r.village.bonus}/tick</span> · unsupported, a Farm link adds +{r.village.farm_bonus}</>}</div>)}
        {c.building === 3 && <div className="text-muted-foreground">{usesResource(r) ? <>+1 Resource in {c.remaining} ticks<br />Adjacent builds: −{r.quarry.troop_discount_pct}% troops, −{r.quarry.resource_discount} Resource</> : <>Adjacent Farms, Villages and Walls cost {r.quarry.troop_discount_pct}% fewer troops{qb > 0 && <><br />Boosting {linked} adjacent Village{linked === 1 ? '' : 's'} by +{qb}/tick each</>}</>}</div>}
        {c.building === 4 && <div className="text-muted-foreground">{wallLinks}/{r.wall.max_links} links · <span className="text-foreground">{(r.wall.defense_milli_by_links[wallLinks ?? 0] / 1000).toFixed(1)}× wall</span> · {cd}</div>}
      </div>
    </div>
  )
}

const StaticLegend = memo(Legend)

const MatchReport = memo(function Report({ rep }: { rep: ViewerReplay }) {
  const rows = BUILDING_KEYS.map((k, i) => ({ k, name: BUILDING_NAMES[i], r: rep.building_report[k] }))
  type Line = [string, (x: ViewerReplay['building_report']['farm']) => string]
  const resourceLine: Line[] = usesResource(rep.rules) ? [['Resource made', (x) => x.avg_resource_generated.toFixed(1)]] : []
  const lines: Line[] = [
    ['Built', (x) => String(x.constructed)],
    ['Avg tick built', (x) => x.avg_construction_tick.toFixed(0)],
    ['Avg troops paid', (x) => x.avg_troops_paid.toFixed(0)],
    ['Captured', (x) => `${(x.captured_share * 100).toFixed(0)}%`],
    ...resourceLine,
    ['Village bonus', (x) => x.avg_village_bonus_troops.toFixed(0)],
    ['Linked time', (x) => `${(x.linked_share * 100).toFixed(0)}%`],
    ['Avg wall ×', (x) => x.avg_wall_multiplier.toFixed(2)],
    ['Repaid', (x) => `${(x.repaid_share * 100).toFixed(0)}%`],
    ['Builder place', (x) => x.avg_builder_place.toFixed(2)],
  ]
  return (
    <Card size="sm">
      <CardHeader><CardTitle>Building instrumentation</CardTitle></CardHeader>
      <CardContent>
        <Table>
          <TableHeader><TableRow className="hover:bg-transparent"><TableHead className="h-7" />{rows.map((x) => <TableHead key={x.k} className="h-7 text-right">{x.name}</TableHead>)}</TableRow></TableHeader>
          <TableBody>
            {lines.map(([label, f]) => (
              <TableRow key={label} className="hover:bg-transparent">
                <TableCell className="py-1 text-muted-foreground">{label}</TableCell>
                {rows.map((x) => <TableCell key={x.k} className="py-1 text-right tabular-nums">{x.r.constructed ? f(x.r) : '—'}</TableCell>)}
              </TableRow>
            ))}
          </TableBody>
        </Table>
        <div className="mt-2 text-[11px] text-muted-foreground">Provisional evidence for this match. Aggregate across self-play with <code>hexarena report</code>.</div>
      </CardContent>
    </Card>
  )
})
