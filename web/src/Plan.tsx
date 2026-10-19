import { ArrowRight, Ban, Hammer, Hourglass, Magnet, Pickaxe } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { BuildingIcon } from './Legend'
import type { Hex, ViewerReplay } from './model'
import { BUILDING_KEYS, BUILDING_NAMES, PLAYER_COLORS } from './model'

/** What one player did on one tick, read back from that tick's events. */
export type PlayerAction =
  | { kind: 'move'; from: Hex; to: Hex; troops: number; outcome: Outcome | null }
  | { kind: 'gather'; target: Hex; sources: number }
  | { kind: 'build'; at: Hex; building: string }
  | { kind: 'demolish'; at: Hex; building: string }
  | { kind: 'rejected'; reason: string }
  | { kind: 'pass' }

/** `sealed`: hexes the capture enclosed and took with it (Go-style capture, v0.7). */
type Outcome = { type: 'captured'; previousOwner: number | null; building: string | null; left: number; sealed: number } | { type: 'repelled'; lost: number }

const same = (a: Hex, b: Hex) => a.q === b.q && a.r === b.r

/** The action `player` took on `tick` (the step from frame `tick - 1` to `tick`). */
export function actionAt(rep: ViewerReplay, tick: number, player: number): PlayerAction {
  const evs = rep.events[tick] ?? []
  for (const e of evs) if (e.type === 'gathered' && e.player === player) return { kind: 'gather', target: e.target, sources: e.sources }
  for (const e of evs) if (e.type === 'built' && e.player === player) return { kind: 'build', at: e.at, building: e.building }
  for (const e of evs) if (e.type === 'demolished' && e.player === player) return { kind: 'demolish', at: e.at, building: e.building }
  for (const e of evs) {
    if (e.type !== 'moved' || e.player !== player) continue
    let outcome: Outcome | null = null
    let sealed = 0
    for (const o of evs) {
      if (o.type === 'surrendered' && o.capturer === player) sealed += o.hexes.length
      if (o.type === 'captured' && o.player === player && !same(o.at, e.to)) sealed++
    }
    for (const o of evs) {
      if (o.type === 'captured' && o.player === player && same(o.at, e.to)) outcome = { type: 'captured', previousOwner: o.previous_owner, building: o.building, left: o.troops_left, sealed }
      if (o.type === 'repelled' && o.attacker === player && same(o.at, e.to)) outcome = { type: 'repelled', lost: o.attackers_lost }
    }
    return { kind: 'move', from: e.from, to: e.to, troops: e.troops, outcome }
  }
  for (const e of evs) if (e.type === 'rejected' && e.player === player) return { kind: 'rejected', reason: e.reason }
  return { kind: 'pass' }
}

const at = (h: Hex) => `(${h.q}, ${h.r})`
const buildingName = (b: string) => BUILDING_NAMES[BUILDING_KEYS.indexOf(b as typeof BUILDING_KEYS[number])] ?? b

function summary(a: PlayerAction, names: string[]): { title: string; detail?: string } {
  switch (a.kind) {
    case 'move': {
      const o = a.outcome
      const detail = !o ? 'into its own land'
        : o.type === 'repelled' ? `attack repelled, ${o.lost} lost`
        : `takes ${o.previousOwner === null ? 'a neutral hex' : `a hex from ${names[o.previousOwner]}`}${o.building ? ` with its ${buildingName(o.building)}` : ''}, ${o.left} left on it${o.sealed ? `, and seals off ${o.sealed} more hex${o.sealed === 1 ? '' : 'es'}` : ''}`
      return { title: `Move ${a.troops} troops ${at(a.from)} to ${at(a.to)}`, detail }
    }
    case 'gather': return { title: `Gather toward ${at(a.target)}`, detail: `${a.sources} hexes step one hex closer` }
    case 'build': return { title: `Build a ${buildingName(a.building)} at ${at(a.at)}` }
    case 'demolish': return { title: `Demolish the ${buildingName(a.building)} at ${at(a.at)}` }
    case 'rejected': return { title: 'Command rejected', detail: a.reason }
    case 'pass': return { title: 'Pass' }
    default: { const never: never = a; return never }
  }
}

function ActionIcon({ a }: { a: PlayerAction }) {
  switch (a.kind) {
    case 'move': return <ArrowRight className="size-3.5" />
    case 'gather': return <Magnet className="size-3.5" />
    case 'build': return <BuildingIcon kind={BUILDING_KEYS.indexOf(a.building as typeof BUILDING_KEYS[number]) + 1} size={16} />
    case 'demolish': return <Pickaxe className="size-3.5" />
    case 'rejected': return <Ban className="size-3.5" />
    case 'pass': return <Hourglass className="size-3.5" />
    default: { const never: never = a; return never }
  }
}

const RECENT = 6
const LOOKBACK = 300

/** The followed player's next action (the one that plays out on the next tick) and its last few. */
export function NextAction({ rep, tick, player, onJump }: { rep: ViewerReplay; tick: number; player: number; onJump: (tick: number) => void }) {
  const last = rep.store.last
  const color = PLAYER_COLORS[player % 8]
  const next = tick < last ? actionAt(rep, tick + 1, player) : null
  const recent: { t: number; a: PlayerAction }[] = []
  for (let t = tick; t >= Math.max(1, tick - LOOKBACK) && recent.length < RECENT; t--) {
    const a = actionAt(rep, t, player)
    if (a.kind !== 'pass') recent.push({ t, a })
  }
  const s = next && summary(next, rep.names)
  return (
    <Card size="sm" className="gap-2 py-3">
      <CardHeader className="flex items-center justify-between">
        <CardTitle className="flex items-center gap-2 text-sm"><Hammer className="size-3.5" style={{ color }} />Up next for {rep.names[player]}</CardTitle>
        {next && <span className="text-[11px] tabular-nums text-muted-foreground">t{tick + 1}</span>}
      </CardHeader>
      <CardContent className="space-y-3">
        {!s ? <p className="text-xs text-muted-foreground">The game is over.</p> : (
          <div className="flex items-start gap-2 rounded-lg border px-2.5 py-2" style={{ borderColor: `${color}66`, background: `${color}12` }}>
            <span className="mt-0.5" style={{ color }}><ActionIcon a={next!} /></span>
            <div className="min-w-0 text-xs">
              <div className="font-medium">{s.title}</div>
              {s.detail && <div className="text-muted-foreground">{s.detail}</div>}
            </div>
          </div>
        )}
        {recent.length > 0 && (
          <div className="space-y-1">
            <div className="text-[10px] font-medium uppercase tracking-[0.14em] text-muted-foreground">Before that</div>
            <ul className="space-y-0.5">
              {recent.map(({ t, a }) => {
                const r = summary(a, rep.names)
                return (
                  <li key={t}>
                    <button type="button" onClick={() => onJump(t - 1)} title="Jump to just before this action"
                      className="flex w-full cursor-pointer items-start gap-2 rounded-md px-1.5 py-1 text-left text-xs hover:bg-muted/40">
                      <span className="mt-0.5 text-muted-foreground"><ActionIcon a={a} /></span>
                      <span className="min-w-0 flex-1"><span className="block truncate">{r.title}</span>{r.detail && <span className="block truncate text-[11px] text-muted-foreground">{r.detail}</span>}</span>
                      <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">t{t}</span>
                    </button>
                  </li>
                )
              })}
            </ul>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
