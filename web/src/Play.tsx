import {
  ArrowLeft, ArrowRight, Bot, Crosshair, Flag, Gamepad2, Globe2, Loader2, Magnet, Pause, Pickaxe, Play, Shuffle, Swords, Trash2, Trophy, Undo2,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api } from './api'
import type { LiveLevel, LiveOrder, LivePhase, LiveSetup, LiveState, LiveSummary, MapInfo, MapPreset } from './api'
import { Board } from './Board'
import { BuildingIcon } from './Legend'
import { Offline, Page } from './Page'
import type { PlayerAction } from './Plan'
import { usePoll } from './poll'
import { BUILDING_KEYS, BUILDING_NAMES, HEIGHT_STYLE, MAP_NAMES, PLAYER_COLORS, buildPreview, hexAt, hexDist, hexIndex } from './model'
import type { BoardRep, Cell, GameEvent, Hex } from './model'

const go = (hash: string) => { location.hash = hash }

const LEVELS: { id: LiveLevel; name: string; blurb: string }[] = [
  { id: 'easy', name: 'Easy', blurb: 'The four original CPUs: Expander, Homesteader, Raider and Warden.' },
  { id: 'medium', name: 'Medium', blurb: 'Bots from the middle of the arena standings.' },
  { id: 'hard', name: 'Hard', blurb: 'The current top of the arena, agent-designed and tuned in self-play.' },
]

const PROCEDURAL = { id: null, name: 'Random', blurb: "The arena's procedural map, new every game." }

const mapName = (m: MapPreset | null) => (m ? MAP_NAMES[m] : 'Random map')

const SETUP_SPEEDS = [1, 2, 5, 10]
const POLL_MS = 200
const LOG_LINES = 80

const PHASE_TEXT: Record<LivePhase, string> = { playing: 'Playing', won: 'Won', lost: 'Lost', cutoff: 'Time ran out', abandoned: 'Abandoned' }

function phaseBadge(p: LivePhase) {
  switch (p) {
    case 'playing': return <Badge className="bg-emerald-500/15 text-emerald-300">{PHASE_TEXT[p]}</Badge>
    case 'won': return <Badge className="bg-amber-400/15 text-amber-300">{PHASE_TEXT[p]}</Badge>
    case 'lost': return <Badge className="bg-rose-500/15 text-rose-300">{PHASE_TEXT[p]}</Badge>
    case 'cutoff': return <Badge variant="secondary">{PHASE_TEXT[p]}</Badge>
    case 'abandoned': return <Badge variant="outline">{PHASE_TEXT[p]}</Badge>
    default: { const never: never = p; return never }
  }
}

function Choice({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button type="button" onClick={onClick} aria-pressed={active}
      className={`flex w-full cursor-pointer flex-col gap-1 rounded-xl border p-3 text-left transition-colors ${active ? 'border-foreground/60 bg-muted/60' : 'hover:bg-muted/30'}`}>
      {children}
    </button>
  )
}

function Label({ children }: { children: React.ReactNode }) {
  return <div className="text-[10px] font-medium uppercase tracking-[0.14em] text-muted-foreground">{children}</div>
}

/** Pick a map, opponents and difficulty, then start a game; also lists this server's games. */
export function PlayPage() {
  const [map, setMap] = useState<MapPreset | null>('europe')
  const [opponents, setOpponents] = useState(3)
  const [level, setLevel] = useState<LiveLevel>('easy')
  const [speed, setSpeed] = useState(5)
  const [autopilot, setAutopilot] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const games = usePoll(api.liveGames, 5000)
  const [maps, setMaps] = useState<MapInfo[] | null>(null)
  const [mapsError, setMapsError] = useState(false)
  useEffect(() => { api.maps().then(setMaps).catch(() => setMapsError(true)) }, [])

  const start = async () => {
    setBusy(true); setError(null)
    try {
      const g = await api.startLive({ map, level, opponents, speed })
      if (!autopilot) await api.liveControl(g.id, { assist: 'off' })
      go(`/play/${g.id}`)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Page icon={Gamepad2} title="Play" description="Take a seat against the arena's CPUs. You give the same one order per tick a bot does; queue several and they run in turn.">
      <div className="grid gap-4 lg:grid-cols-[1fr_20rem]">
        <Card>
          <CardHeader>
            <CardTitle>New game</CardTitle>
            <CardDescription>Every real map is one connected landmass with no islands; straits are bridged. Your start city is picked at random.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            <div className="space-y-2">
              <Label>Map</Label>
              {!maps && !mapsError && <div className="flex items-center gap-2 text-xs text-muted-foreground"><Loader2 className="size-3.5 animate-spin" />Loading maps…</div>}
              {mapsError && <p className="text-xs text-rose-400">Couldn't load the real-world maps; only the random map is available.</p>}
              <div className="grid gap-2 sm:grid-cols-2">
                {[...(maps ?? []), PROCEDURAL].map((m) => (
                  <Choice key={m.name} active={map === m.id} onClick={() => setMap(m.id)}>
                    <span className="flex items-center gap-2 text-sm font-medium">
                      {m.id ? <Globe2 className="size-4 text-muted-foreground" /> : <Shuffle className="size-4 text-muted-foreground" />}{m.name}
                    </span>
                    <span className="text-xs text-muted-foreground">{m.blurb}</span>
                  </Choice>
                ))}
              </div>
            </div>
            <div className="space-y-2">
              <Label>CPU opponents</Label>
              <div className="flex flex-wrap items-center gap-3">
                <ToggleGroup variant="outline" size="sm" value={[String(opponents)]} onValueChange={(v) => v.length && setOpponents(Number(v[v.length - 1]))}>
                  {[1, 2, 3, 4, 5, 6, 7].map((n) => <ToggleGroupItem key={n} value={String(n)} className="w-9 data-pressed:bg-foreground data-pressed:text-background">{n}</ToggleGroupItem>)}
                </ToggleGroup>
                <span className="text-xs text-muted-foreground">{opponents === 1 ? 'A 1v1 duel' : `A ${opponents + 1}-player free-for-all`}</span>
              </div>
            </div>
            <div className="space-y-2">
              <Label>Difficulty</Label>
              <div className="grid gap-2 sm:grid-cols-3">
                {LEVELS.map((l) => (
                  <Choice key={l.id} active={level === l.id} onClick={() => setLevel(l.id)}>
                    <span className="text-sm font-medium">{l.name}</span>
                    <span className="text-xs text-muted-foreground">{l.blurb}</span>
                  </Choice>
                ))}
              </div>
            </div>
            <div className="flex flex-wrap gap-6">
              <div className="space-y-2">
                <Label>Pace</Label>
                <ToggleGroup variant="outline" size="sm" value={[String(speed)]} onValueChange={(v) => v.length && setSpeed(Number(v[v.length - 1]))}>
                  {SETUP_SPEEDS.map((s) => <ToggleGroupItem key={s} value={String(s)} className="px-2 text-xs data-pressed:bg-foreground data-pressed:text-background">{s} tick{s === 1 ? '' : 's'}/s</ToggleGroupItem>)}
                </ToggleGroup>
              </div>
              <div className="space-y-2">
                <Label>Autopilot</Label>
                <ToggleGroup variant="outline" size="sm" value={[autopilot ? 'on' : 'off']} onValueChange={(v) => v.length && setAutopilot(v[v.length - 1] === 'on')}>
                  <ToggleGroupItem value="on" className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">On</ToggleGroupItem>
                  <ToggleGroupItem value="off" className="px-2.5 text-xs data-pressed:bg-foreground data-pressed:text-background">Off</ToggleGroupItem>
                </ToggleGroup>
              </div>
            </div>
            <p className="text-xs text-muted-foreground">
              {autopilot
                ? 'Autopilot plays your seat on any tick your order queue is empty, so you can step in and out. You can switch it off or change its style during the game.'
                : 'Every tick without an order of yours is a pass. The CPUs act every tick, so queue moves ahead or slow the pace.'}
            </p>
            {error && <p className="text-sm text-rose-400">{error}</p>}
            <Button size="lg" onClick={start} disabled={busy} className="w-full sm:w-auto">
              {busy ? <Loader2 className="animate-spin" /> : <Swords />}Start game
            </Button>
          </CardContent>
        </Card>
        <div className="space-y-4">
          <Card size="sm">
            <CardHeader><CardTitle>How to play</CardTitle></CardHeader>
            <CardContent>
              <ul className="space-y-1.5 text-xs text-muted-foreground">
                <li><b className="text-foreground">Click</b> your hex to select it, then click a neighbour to send its spare troops there. Keep clicking to chain moves.</li>
                <li><b className="text-foreground">Shift-click</b> or <b className="text-foreground">right-click</b> one of your hexes to gather troops toward it.</li>
                <li><b className="text-foreground">1–4</b> build a Farm, Village, Quarry or Wall on the selected hex. <b className="text-foreground">X</b> demolishes.</li>
                <li><b className="text-foreground">H</b> switches between sending all or half. <b className="text-foreground">Z</b> undoes the last order. <b className="text-foreground">Space</b> pauses.</li>
                <li>Seal off a region with your land and it's yours. Own the whole map to win.</li>
              </ul>
            </CardContent>
          </Card>
          <Card size="sm">
            <CardHeader><CardTitle>Your games</CardTitle></CardHeader>
            <CardContent>
              {games.error && !games.data ? <Offline error={games.error} onRetry={games.refresh} />
                : !games.data ? <div className="flex items-center gap-2 text-xs text-muted-foreground"><Loader2 className="size-3.5 animate-spin" />Loading…</div>
                : games.data.length === 0 ? <p className="text-xs text-muted-foreground">No games yet. The ones you start show up here while the server keeps running.</p>
                : <ul className="space-y-1.5">{games.data.map((g) => <GameRow key={g.id} g={g} />)}</ul>}
            </CardContent>
          </Card>
        </div>
      </div>
    </Page>
  )
}

function GameRow({ g }: { g: LiveSummary }) {
  const rivals = g.seats.filter((_, i) => i !== g.you).length
  const playing = g.phase === 'playing'
  return (
    <li className="flex items-center gap-2 rounded-lg border px-2.5 py-2 text-xs">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 font-medium">{mapName(g.map)} {phaseBadge(g.phase)}</div>
        <div className="truncate text-muted-foreground">{rivals} CPU{rivals === 1 ? '' : 's'} · tick {g.tick} · <span className="font-mono">{g.id}</span></div>
      </div>
      <Button size="xs" variant={playing ? 'default' : 'outline'} onClick={() => go(playing ? `/play/${g.id}` : `/match/${g.id}`)}>
        {playing ? 'Continue' : 'Replay'}
      </Button>
    </li>
  )
}

/** One line of the game log; `tone` colours it. */
interface Line { tick: number; text: string; tone: 'good' | 'bad' | 'info' }

const kindOf = (name: string) => BUILDING_KEYS.indexOf(name as typeof BUILDING_KEYS[number]) + 1
const buildingName = (name: string) => BUILDING_NAMES[kindOf(name) - 1] ?? name

/** What the log says about an event the player saw; null for the routine ones. */
function logLine(e: GameEvent, you: number, names: string[], mine: (h: Hex) => boolean): Omit<Line, 'tick'> | null {
  const who = (p: number) => (p === you ? 'You' : names[p])
  switch (e.type) {
    case 'captured':
      if (e.player === you) return { tone: 'good', text: `You took ${e.previous_owner === null ? 'a neutral hex' : `a hex from ${names[e.previous_owner]}`}${e.building ? ` with its ${buildingName(e.building)}` : ''}` }
      if (e.previous_owner === you) return { tone: 'bad', text: `${names[e.player]} took one of your hexes${e.building ? ` and its ${buildingName(e.building)}` : ''}` }
      return e.previous_owner === null ? null : { tone: 'info', text: `${names[e.player]} took a hex from ${names[e.previous_owner]}` }
    case 'repelled':
      if (e.attacker === you) return { tone: 'bad', text: `Your attack was repelled; ${e.attackers_lost} lost` }
      return mine(e.at) ? { tone: 'good', text: `You held off ${names[e.attacker]}` } : null
    case 'surrendered':
      if (e.capturer === you) return { tone: 'good', text: `You sealed off ${e.hexes.length} hex${e.hexes.length === 1 ? '' : 'es'} of ${names[e.from]}` }
      if (e.from === you) return { tone: 'bad', text: `${names[e.capturer]} sealed off ${e.hexes.length} of your hexes` }
      return { tone: 'info', text: `${names[e.capturer]} sealed off ${e.hexes.length} hexes of ${names[e.from]}` }
    case 'built': return { tone: e.player === you ? 'good' : 'info', text: `${who(e.player)} built a ${buildingName(e.building)}` }
    case 'demolished': return { tone: 'info', text: `${who(e.player)} demolished a ${buildingName(e.building)}` }
    case 'eliminated': return e.player === you ? { tone: 'bad', text: 'You were eliminated' } : { tone: 'info', text: `${names[e.player]} was eliminated` }
    case 'rejected': return e.player === you ? { tone: 'bad', text: `Order refused: ${e.reason}` } : null
    case 'gathered': return e.player === you ? { tone: 'info', text: `You gathered troops from ${e.sources} hexes` } : null
    case 'moved': case 'link_changed': case 'links_broken': return null
    default: { const never: never = e; return never }
  }
}

function boardRep(s: LiveSetup): BoardRep {
  return { radius: s.radius, fog: s.fog, names: s.names, rules: s.rules, terrain: s.terrain, neutral: new Array((2 * s.radius + 1) ** 2).fill(0), events: [] }
}

function decodeCells(rec: number[]): Map<number, Cell> {
  const m = new Map<number, Cell>()
  for (let i = 0; i < rec.length; i += 9)
    m.set(rec[i], { owner: rec[i + 1], troops: rec[i + 2], building: rec[i + 3], remaining: rec[i + 4], links: [rec[i + 5], rec[i + 6], rec[i + 7]], cooldown: rec[i + 8] })
  return m
}

const same = (a: Hex, b: Hex) => a.q === b.q && a.r === b.r

function orderAction(o: LiveOrder): PlayerAction {
  switch (o.type) {
    case 'move': return { kind: 'move', from: o.from, to: o.to, troops: 0, outcome: null }
    case 'gather': return { kind: 'gather', target: o.target, sources: 0 }
    case 'build': return { kind: 'build', at: o.at, building: o.building }
    case 'demolish': return { kind: 'demolish', at: o.at, building: '' }
    default: { const never: never = o; return never }
  }
}

function OrderText({ o, place }: { o: LiveOrder; place: (h: Hex) => string }) {
  switch (o.type) {
    case 'move': return <><ArrowRight className="size-3.5 shrink-0 text-muted-foreground" />Send {o.share_pct === 100 ? 'all' : `${o.share_pct}%`} from {place(o.from)} to {place(o.to)}</>
    case 'gather': return <><Magnet className="size-3.5 shrink-0 text-muted-foreground" />Gather toward {place(o.target)}</>
    case 'build': return <><BuildingIcon kind={kindOf(o.building)} size={16} />Build a {buildingName(o.building)} at {place(o.at)}</>
    case 'demolish': return <><Pickaxe className="size-3.5 shrink-0 text-muted-foreground" />Demolish at {place(o.at)}</>
    default: { const never: never = o; return never }
  }
}

/** Polls a live game: its fixed setup once, then the player's view, keeping the seen events in `rep`. */
function useLive(id: string) {
  const [setup, setSetup] = useState<LiveSetup | null>(null)
  const [state, setState] = useState<LiveState | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [lines, setLines] = useState<Line[]>([])
  const rep = useMemo(() => setup && boardRep(setup), [setup])
  const since = useRef(0)

  useEffect(() => {
    api.liveSetup(id).then(setSetup).catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [id])

  useEffect(() => {
    if (!rep || !setup) return
    let stop = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const poll = async () => {
      try {
        const s = await api.liveState(id, since.current)
        if (stop) return
        const cells = decodeCells(s.cells)
        const mine = (h: Hex) => cells.get(hexIndex(rep.radius, h))?.owner === setup.you
        const fresh: Line[] = []
        for (const [t, evs] of s.events) {
          rep.events[t] = evs
          for (const e of evs) {
            const l = logLine(e, setup.you, setup.names, mine)
            if (l) fresh.push({ tick: t, ...l })
          }
        }
        if (fresh.length) setLines((old) => [...fresh.reverse(), ...old].slice(0, LOG_LINES))
        since.current = Math.max(since.current, s.tick)
        setState(s)
        setError(null)
        if (s.phase === 'playing') timer = setTimeout(poll, POLL_MS)
      } catch (e) {
        if (stop) return
        setError(e instanceof Error ? e.message : String(e))
        timer = setTimeout(poll, 2000)
      }
    }
    poll()
    return () => { stop = true; clearTimeout(timer) }
  }, [id, rep, setup])

  return { setup, rep, state, setState, lines, error }
}

/** A game in progress: the board in your fog, your order queue, and the pace controls. */
export function LiveGame({ id }: { id: string }) {
  const { setup, rep, state, setState, lines, error } = useLive(id)
  if (error && !setup) {
    return (
      <div className="mx-auto max-w-lg p-6">
        <Card>
          <CardHeader>
            <CardTitle>This game isn't running</CardTitle>
            <CardDescription>Live games only last while the server runs; finished ones stay watchable as replays. ({error})</CardDescription>
          </CardHeader>
          <CardContent className="flex gap-2">
            <Button size="sm" onClick={() => go('/play')}>New game</Button>
            <Button size="sm" variant="outline" onClick={() => go(`/match/${id}`)}>Try the replay</Button>
          </CardContent>
        </Card>
      </div>
    )
  }
  if (!setup || !rep || !state) {
    return <div className="flex h-full items-center justify-center gap-2 text-muted-foreground"><Loader2 className="size-4 animate-spin" />Setting up the board…</div>
  }
  return <LiveTable id={id} setup={setup} rep={rep} state={state} lines={lines} offline={error} onQueue={(queue) => setState((s) => (s ? { ...s, queue } : s))}
    onControl={(c) => setState((s) => (s ? { ...s, ...c } : s))} />
}

type Control = { speed?: number; paused?: boolean; assist?: string; resign?: boolean }
type Controlled = Pick<LiveState, 'speed' | 'paused' | 'assist' | 'phase'>

/** What the hex panel's buttons do. */
interface Act {
  build: (kind: number) => void
  demolish: () => void
  gather: (idx: number) => void
  send: (orders: LiveOrder[], opts?: { clear?: boolean; undo?: boolean }) => void
  setShare: (pct: number) => void
}

function homeOf(cells: Map<number, Cell>, you: number): number | null {
  for (const [i, c] of cells) if (c.owner === you && c.building === 2) return i
  for (const [i, c] of cells) if (c.owner === you) return i
  return null
}

const FOCUS_ZOOM = 1.1

function LiveTable({ id, setup, rep, state, lines, offline, onQueue, onControl }: {
  id: string; setup: LiveSetup; rep: BoardRep; state: LiveState; lines: Line[]; offline: string | null
  onQueue: (q: LiveOrder[]) => void; onControl: (c: Controlled) => void
}) {
  const you = setup.you
  const cells = useMemo(() => decodeCells(state.cells), [state])
  const visible = useMemo(() => new Set(state.visible), [state])
  const [selected, setSelected] = useState<number | null>(() => homeOf(cells, you))
  const [focus, setFocus] = useState<{ idx: number; k: number } | null>(() => { const h = homeOf(cells, you); return h === null ? null : { idx: h, k: FOCUS_ZOOM } })
  const [share, setShare] = useState(100)
  const [hint, setHint] = useState<string | null>(null)
  const [confirmResign, setConfirmResign] = useState(false)
  const over = state.phase !== 'playing'
  const queue = state.queue
  const placeAt = useMemo(() => new Map(setup.places.map((p) => [p.idx, p.name])), [setup])
  const totalLand = useMemo(() => rep.terrain.filter((t) => t?.kind === 'open').length, [rep])
  const home = homeOf(cells, you)

  useEffect(() => {
    if (!hint) return
    const t = setTimeout(() => setHint(null), 2600)
    return () => clearTimeout(t)
  }, [hint])

  const plans = useMemo(() => queue.map((o) => {
    const action = orderAction(o)
    if (o.type !== 'move') return { player: you, action }
    const src = cells.get(hexIndex(rep.radius, o.from))
    const label = src?.owner === you ? String(Math.ceil((Math.max(0, src.troops - 1) * o.share_pct) / 100)) : o.share_pct === 100 ? 'all' : '½'
    return { player: you, action, label }
  }), [queue, cells, rep, you])

  const fail = (e: unknown) => setHint(e instanceof Error ? e.message : String(e))
  const send = (orders: LiveOrder[], opts: { clear?: boolean; undo?: boolean } = {}) => {
    api.liveOrders(id, orders, opts).then((r) => onQueue(r.queue)).catch(fail)
  }
  const control = (c: Control) => { api.liveControl(id, c).then(onControl).catch(fail) }

  const hexOf = (i: number) => hexAt(rep.radius, i)
  const place = (h: Hex) => placeAt.get(hexIndex(rep.radius, h)) ?? `(${h.q}, ${h.r})`
  const isMine = (i: number) => cells.get(i)?.owner === you
  /** A hex orders can start from: yours now, or where a queued move will have put troops. */
  const canOrderFrom = (i: number) => isMine(i) || queue.some((o) => o.type === 'move' && same(o.to, hexOf(i)))

  const act: Act = {
    gather: (i) => {
      if (!isMine(i)) { setHint('Gather toward one of your own hexes'); return }
      send([{ type: 'gather', target: hexOf(i) }])
    },
    build: (kind) => {
      const name = BUILDING_NAMES[kind - 1]
      if (selected === null || !isMine(selected)) { setHint('Select one of your hexes first'); return }
      const p = buildPreview(rep, cells, selected, kind)
      if (!p.legal) { setHint(`Can't build a ${name}: ${p.reason}`); return }
      const troops = cells.get(selected)?.troops ?? 0
      if (troops < p.troops + 1) { setHint(`A ${name} costs ${p.troops} troops and must leave 1; this hex has ${troops}`); return }
      send([{ type: 'build', at: hexOf(selected), building: BUILDING_KEYS[kind - 1] }])
    },
    demolish: () => {
      if (selected === null || !isMine(selected) || !cells.get(selected)?.building) { setHint('Select one of your buildings to demolish'); return }
      send([{ type: 'demolish', at: hexOf(selected) }])
    },
    send,
    setShare,
  }

  const onSelect = (i: number | null, mods?: { alt: boolean }) => {
    if (i === null || over) { setSelected(i); return }
    if (mods?.alt) { act.gather(i); return }
    if (selected !== null && i !== selected && hexDist(hexOf(selected), hexOf(i)) === 1 && canOrderFrom(selected)) {
      const t = rep.terrain[i]
      if (!t || t.kind !== 'open') { setHint(t?.kind === 'water' ? "Troops can't cross the sea" : 'Mountains are impassable'); return }
      send([{ type: 'move', from: hexOf(selected), to: hexOf(i), share_pct: share }])
      setSelected(i)
      return
    }
    setSelected(i === selected ? null : i)
  }

  const onKey = (e: KeyboardEvent) => {
    if (over) return
    if (e.key >= '1' && e.key <= '4') act.build(Number(e.key))
    else if (e.key === 'x' || e.key === 'X') act.demolish()
    else if (e.key === 'g' || e.key === 'G') { if (selected !== null) act.gather(selected) }
    else if (e.key === 'h' || e.key === 'H') setShare((s) => (s === 100 ? 50 : 100))
    else if (e.key === 'z' || e.key === 'Z' || e.key === 'Backspace') { if (queue.length) send([], { undo: true }) }
    else if (e.key === ' ') control({ paused: !state.paused })
    else if (e.key === 'Escape') setSelected(null)
    else return
    e.preventDefault()
  }
  const latestKey = useRef(onKey)
  useEffect(() => { latestKey.current = onKey })
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null
      if (e.metaKey || e.ctrlKey || e.altKey || el?.closest('input, textarea, select, [role="combobox"], [role="listbox"]')) return
      latestKey.current(e)
    }
    window.addEventListener('keydown', on)
    return () => window.removeEventListener('keydown', on)
  }, [])

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto bg-background p-3 text-foreground lg:flex-row lg:overflow-hidden">
      <div className="relative h-[62svh] shrink-0 lg:h-auto lg:min-h-0 lg:flex-1">
        <Board rep={rep} cells={cells} perspective={you} visible={visible} selected={selected} heightMode={false} tick={state.tick}
          plans={over ? [] : plans} labels={setup.places} calmSea={setup.map !== null} focus={focus} onSelect={onSelect} />
        <div className="pointer-events-none absolute inset-x-0 top-12 flex justify-center px-4">
          {hint || offline ? <div className="rounded-lg border border-amber-400/40 bg-background/90 px-3 py-1.5 text-xs text-amber-200 shadow backdrop-blur">{hint ?? `Lost the server: ${offline}`}</div>
            : !over && <div className="hidden rounded-lg border bg-background/70 px-3 py-1.5 text-[11px] text-muted-foreground backdrop-blur md:block">
              Click your hex, then a neighbour to send troops · Shift/right-click: gather · 1–4 build · X demolish · H all/half · Z undo · Space pause
            </div>}
        </div>
        {over && <GameOver id={id} setup={setup} state={state} />}
      </div>
      <aside className="flex w-full shrink-0 flex-col gap-3 lg:w-[22rem] lg:overflow-y-auto">
        <Card size="sm" className="gap-3 py-3">
          <CardHeader className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2 text-sm">
              <span className="size-3 rounded-full" style={{ background: PLAYER_COLORS[you % 8] }} />
              {mapName(setup.map)} · {setup.names.length} players
            </CardTitle>
            {phaseBadge(state.phase)}
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="space-y-1">
              <div className="flex justify-between text-xs tabular-nums text-muted-foreground">
                <span>Tick {state.tick.toLocaleString()} of {state.max_ticks.toLocaleString()}</span><span>{state.land[you] ?? 0} of {totalLand} hexes yours</span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-foreground/70" style={{ width: `${(100 * state.tick) / state.max_ticks}%` }} /></div>
            </div>
            {!over && (
              <>
                <div className="flex flex-wrap items-center gap-2">
                  <Button size="sm" variant={state.paused ? 'default' : 'secondary'} onClick={() => control({ paused: !state.paused })}>
                    {state.paused ? <Play /> : <Pause />}{state.paused ? 'Resume' : 'Pause'}
                  </Button>
                  <ToggleGroup variant="outline" size="sm" value={[String(state.speed)]} onValueChange={(v) => v.length && control({ speed: Number(v[v.length - 1]) })}>
                    {setup.speeds.map((s) => <ToggleGroupItem key={s} value={String(s)} className="px-1.5 text-[11px] data-pressed:bg-foreground data-pressed:text-background">{s}/s</ToggleGroupItem>)}
                  </ToggleGroup>
                </div>
                <div className="space-y-1">
                  <Label>Autopilot</Label>
                  <Select value={state.assist ?? 'off'} onValueChange={(v) => v && control({ assist: String(v) })}
                    items={[{ value: 'off', label: 'Off: you play every tick' }, ...setup.assists.map((a) => ({ value: a.id, label: `${a.name} plays when your queue is empty` }))]}>
                    <SelectTrigger size="sm" className="w-full"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="off">Off: you play every tick</SelectItem>
                      {setup.assists.map((a) => <SelectItem key={a.id} value={a.id} title={a.blurb}>{a.name} plays when your queue is empty</SelectItem>)}
                    </SelectContent>
                  </Select>
                </div>
              </>
            )}
            <div className="flex flex-wrap gap-2">
              {home !== null && <Button size="sm" variant="outline" onClick={() => setFocus({ idx: home, k: FOCUS_ZOOM })}><Crosshair />Find my land</Button>}
              {!over && (confirmResign
                ? <Button size="sm" variant="destructive" onClick={() => { control({ resign: true }); setConfirmResign(false) }}><Flag />Really resign?</Button>
                : <Button size="sm" variant="ghost" onClick={() => setConfirmResign(true)}><Flag />Resign</Button>)}
            </div>
          </CardContent>
        </Card>
        {!over && <SelectedHex setup={setup} rep={rep} cells={cells} visible={visible} selected={selected} share={share} placeAt={placeAt} act={act} />}
        {!over && <QueueCard queue={queue} assist={state.assist} place={place} act={act} />}
        <Players setup={setup} state={state} />
        <Card size="sm" className="gap-2 py-3">
          <CardHeader><CardTitle className="text-sm">What happened</CardTitle></CardHeader>
          <CardContent>
            {lines.length === 0 ? <p className="text-xs text-muted-foreground">Captures, builds and refused orders you can see show up here.</p> : (
              <ul className="max-h-56 space-y-0.5 overflow-y-auto pr-1 text-xs">
                {lines.map((l, i) => (
                  <li key={`${l.tick}-${i}`} className="flex gap-2">
                    <span className="w-11 shrink-0 tabular-nums text-muted-foreground">t{l.tick}</span>
                    <span className={l.tone === 'good' ? 'text-emerald-300' : l.tone === 'bad' ? 'text-rose-300' : 'text-muted-foreground'}>{l.text}</span>
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
      </aside>
    </div>
  )
}

function SelectedHex({ setup, rep, cells, visible, selected, share, placeAt, act }: {
  setup: LiveSetup; rep: BoardRep; cells: Map<number, Cell>; visible: Set<number>; selected: number | null; share: number
  placeAt: Map<number, string>; act: Act
}) {
  if (selected === null) {
    return (
      <Card size="sm" className="py-3">
        <CardContent className="text-xs text-muted-foreground">Click one of your hexes to give it orders, or any hex to inspect it.</CardContent>
      </Card>
    )
  }
  const h = hexAt(rep.radius, selected)
  const t = rep.terrain[selected]
  const c = cells.get(selected)
  const seen = visible.has(selected)
  const mine = c?.owner === setup.you
  const owner = !seen ? 'Out of sight' : c && c.owner >= 0 ? setup.names[c.owner] : 'Neutral'
  const terrain = !t ? '' : t.kind === 'water' ? 'Sea' : t.kind === 'mountain' ? 'Mountains' : HEIGHT_STYLE[t.height].name
  return (
    <Card size="sm" className="gap-2 py-3">
      <CardHeader className="flex items-center justify-between">
        <CardTitle className="text-sm">{placeAt.get(selected) ?? 'Hex'} <span className="font-mono text-xs font-normal text-muted-foreground">({h.q}, {h.r})</span></CardTitle>
        <ToggleGroup variant="outline" size="sm" value={[String(share)]} onValueChange={(v) => v.length && act.setShare(Number(v[v.length - 1]))}>
          <ToggleGroupItem value="100" className="px-2 text-[11px] data-pressed:bg-foreground data-pressed:text-background">Send all</ToggleGroupItem>
          <ToggleGroupItem value="50" className="px-2 text-[11px] data-pressed:bg-foreground data-pressed:text-background">Half</ToggleGroupItem>
        </ToggleGroup>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid grid-cols-3 gap-2 text-xs">
          <div><Label>Terrain</Label><div>{terrain}</div></div>
          <div><Label>Owner</Label><div className="flex items-center gap-1.5">{seen && c && c.owner >= 0 && <span className="size-2 rounded-full" style={{ background: PLAYER_COLORS[c.owner % 8] }} />}{mine ? 'You' : owner}</div></div>
          <div><Label>Troops</Label><div className="tabular-nums">{seen && c ? c.troops : '?'}</div></div>
        </div>
        {mine ? (
          <>
            <div className="grid grid-cols-2 gap-1.5">
              {[1, 2, 3, 4].map((k) => {
                const p = buildPreview(rep, cells, selected, k)
                return (
                  <Button key={k} size="sm" variant="outline" disabled={!p.legal} title={p.reason ?? undefined} className="justify-start" onClick={() => act.build(k)}>
                    <BuildingIcon kind={k} size={16} />{BUILDING_NAMES[k - 1]}<span className="ml-auto text-[11px] tabular-nums text-muted-foreground">{p.troops} · {k}</span>
                  </Button>
                )
              })}
            </div>
            <div className="flex gap-1.5">
              <Button size="sm" variant="outline" className="flex-1" onClick={() => act.gather(selected)}><Magnet />Gather here · G</Button>
              {c && c.building > 0 && <Button size="sm" variant="outline" className="flex-1" onClick={act.demolish}><Pickaxe />Demolish · X</Button>}
            </div>
            <p className="text-[11px] text-muted-foreground">Click a neighbouring hex to send {share === 100 ? 'all spare troops' : 'half the spare troops'} there.</p>
          </>
        ) : (
          <p className="text-[11px] text-muted-foreground">{seen ? 'Not yours. Select one of your hexes next to it and click it to attack.' : 'Your scouts can only see 3 hexes past your border.'}</p>
        )}
      </CardContent>
    </Card>
  )
}

function QueueCard({ queue, assist, place, act }: { queue: LiveOrder[]; assist: string | null; place: (h: Hex) => string; act: Act }) {
  return (
    <Card size="sm" className="gap-2 py-3">
      <CardHeader className="flex items-center justify-between">
        <CardTitle className="text-sm">Orders <span className="font-normal text-muted-foreground">{queue.length ? `${queue.length} waiting` : ''}</span></CardTitle>
        <div className="flex gap-1">
          <Button size="xs" variant="ghost" disabled={!queue.length} onClick={() => act.send([], { undo: true })}><Undo2 />Undo</Button>
          <Button size="xs" variant="ghost" disabled={!queue.length} onClick={() => act.send([], { clear: true })}><Trash2 />Clear</Button>
        </div>
      </CardHeader>
      <CardContent>
        {queue.length === 0
          ? <p className="text-xs text-muted-foreground">{assist ? 'Empty, so autopilot is playing your seat.' : 'Empty: you pass each tick until you give an order.'}</p>
          : (
            <ol className="max-h-40 space-y-0.5 overflow-y-auto pr-1 text-xs">
              {queue.map((o, i) => (
                <li key={i} className={`flex items-center gap-1.5 rounded px-1 py-0.5 ${i === 0 ? 'bg-muted/50' : 'text-muted-foreground'}`}><OrderText o={o} place={place} /></li>
              ))}
            </ol>
          )}
      </CardContent>
    </Card>
  )
}

function Players({ setup, state }: { setup: LiveSetup; state: LiveState }) {
  const order = setup.names.map((_, i) => i).sort((a, b) => (state.land[b] ?? 0) - (state.land[a] ?? 0))
  return (
    <Card size="sm" className="gap-2 py-3">
      <CardHeader><CardTitle className="text-sm">Players</CardTitle></CardHeader>
      <CardContent>
        <ul className="space-y-1 text-xs">
          {order.map((p) => (
            <li key={p} className={`flex items-center gap-2 ${state.alive[p] ? '' : 'text-muted-foreground line-through'}`}>
              <span className="size-2.5 shrink-0 rounded-full" style={{ background: PLAYER_COLORS[p % 8] }} />
              <span className={`min-w-0 flex-1 truncate ${p === setup.you ? 'font-semibold' : ''}`}>{setup.names[p]}{p !== setup.you && <span className="ml-1.5 font-mono text-[10px] text-muted-foreground">{setup.seats[p].bot}</span>}</span>
              <span className="tabular-nums text-muted-foreground">{state.land[p] ?? 0}</span>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  )
}

function GameOver({ id, setup, state }: { id: string; setup: LiveSetup; state: LiveState }) {
  const [hidden, setHidden] = useState(false)
  if (hidden) return null
  const me = state.result?.placements.find((p) => p.player === setup.you)
  const place = me?.place_start
  const icon = state.phase === 'won' ? <Trophy className="size-5 text-amber-300" /> : state.phase === 'lost' ? <Flag className="size-5 text-rose-300" /> : <Bot className="size-5 text-muted-foreground" />
  const title = state.phase === 'won' ? 'You conquered the map'
    : state.phase === 'lost' ? (state.alive[setup.you] ? 'You resigned' : 'You were eliminated')
    : state.phase === 'cutoff' ? `Time ran out: you placed ${place ?? '?'} of ${setup.names.length}`
    : 'This game was abandoned'
  return (
    <div className="absolute inset-0 flex items-center justify-center bg-background/40 p-4 backdrop-blur-[2px]">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">{icon}{title}</CardTitle>
          <CardDescription>
            Tick {state.result?.final_tick ?? state.tick} with {state.land[setup.you] ?? 0} hexes yours.
            {place !== undefined && state.phase !== 'cutoff' && ` Final place ${place} of ${setup.names.length}.`}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button size="sm" onClick={() => go('/play')}><Swords />New game</Button>
          <Button size="sm" variant="outline" onClick={() => go(`/match/${id}`)}>Watch the replay</Button>
          <Button size="sm" variant="ghost" onClick={() => setHidden(true)}><ArrowLeft />See the board</Button>
        </CardContent>
      </Card>
    </div>
  )
}
