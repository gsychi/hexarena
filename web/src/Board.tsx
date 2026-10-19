import { Eye, EyeOff, Maximize2, ZoomIn, ZoomOut } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { BUILDING_KEYS, HEIGHT_STYLE, HEX_DIRS, PLAYER_COLORS, PLAYER_MARKS, addHex, hexAt, hexDist, hexIndex, onBoard, stormFloor, stormRadius } from './model'
import type { BoardRep, Cell, Hex, Rules } from './model'
import { BUILDING_STYLE, drawBuilding, drawHeightMark, drawPlot } from './icons'
import type { Pips } from './icons'
import type { PlayerAction } from './Plan'

const SIZE = 22
const toPx = (h: Hex) => ({ x: SIZE * Math.sqrt(3) * (h.q + h.r / 2), y: SIZE * 1.5 * h.r })

function hexPath(ctx: CanvasRenderingContext2D, x: number, y: number, s: number) {
  const pts = Array.from({ length: 6 }, (_, i) => {
    const a = (Math.PI / 180) * (60 * i - 30)
    return [x + s * Math.cos(a), y + s * Math.sin(a)]
  })
  const r = s * 0.14
  ctx.beginPath()
  const mid = (i: number) => [(pts[i][0] + pts[(i + 1) % 6][0]) / 2, (pts[i][1] + pts[(i + 1) % 6][1]) / 2]
  const m0 = mid(5)
  ctx.moveTo(m0[0], m0[1])
  for (let i = 0; i < 6; i++) {
    const m = mid(i)
    ctx.arcTo(pts[i][0], pts[i][1], m[0], m[1], r)
  }
  ctx.closePath()
}

function outlinedText(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, fill: string) {
  ctx.lineWidth = 2.2; ctx.strokeStyle = 'rgba(6,7,10,0.9)'; ctx.strokeText(text, x, y)
  ctx.fillStyle = fill; ctx.fillText(text, x, y)
}

/** Unit-radius corner offsets of a pointy-top hex, corner i at 60i - 30 degrees. */
const CORNER = Array.from({ length: 6 }, (_, i) => {
  const a = (Math.PI / 180) * (60 * i - 30)
  return [Math.cos(a), Math.sin(a)] as const
})
/** The two corners bounding the edge that faces `HEX_DIRS[d]`. */
const EDGE: [number, number][] = [[0, 1], [5, 0], [4, 5], [3, 4], [2, 3], [1, 2]]

/** A plain hexagon added to the current path; cheap enough to batch thousands. */
function hexPoly(ctx: CanvasRenderingContext2D | Path2D, x: number, y: number, s: number) {
  ctx.moveTo(x + s * CORNER[0][0], y + s * CORNER[0][1])
  for (let i = 1; i < 6; i++) ctx.lineTo(x + s * CORNER[i][0], y + s * CORNER[i][1])
  ctx.closePath()
}

interface Geometry { x: Float32Array; y: Float32Array; nb: Int32Array }

/** Pixel centers and neighbor indices (-1 off board) of every hex, computed once per match. */
function geometry(rep: BoardRep, all: number[]): Geometry {
  const n = (2 * rep.radius + 1) ** 2
  const g = { x: new Float32Array(n), y: new Float32Array(n), nb: new Int32Array(n * 6).fill(-1) }
  for (const idx of all) {
    const h = hexAt(rep.radius, idx)
    const p = toPx(h)
    g.x[idx] = p.x; g.y[idx] = p.y
    HEX_DIRS.forEach((d, k) => {
      const m = addHex(h, d)
      if (onBoard(rep.radius, m) && rep.terrain[hexIndex(rep.radius, m)]) g.nb[idx * 6 + k] = hexIndex(rep.radius, m)
    })
  }
  return g
}

function drawTerrain(ctx: CanvasRenderingContext2D, rep: BoardRep, geo: Geometry, s: number, detail: number, heightMode: boolean, calmSea: boolean) {
  const n = (2 * rep.radius + 1) ** 2
  for (let idx = 0; idx < n; idx++) {
    const t = rep.terrain[idx]
    if (!t) continue
    const x = geo.x[idx], y = geo.y[idx]
    hexPath(ctx, x, y, s)
    if (t.kind === 'water' && calmSea) {
      hexPoly(ctx, x, y, s + 1.8); ctx.fillStyle = '#0b1a2c'; ctx.fill()
      continue
    }
    if (t.kind !== 'open') {
      const water = t.kind === 'water'
      ctx.fillStyle = water ? '#0b1626' : '#14161b'
      ctx.fill()
      ctx.strokeStyle = water ? '#2c6fa3' : '#4a4f5c'
      ctx.lineWidth = 1; ctx.stroke()
      ctx.save(); ctx.clip()
      ctx.beginPath()
      for (let d = -SIZE; d <= SIZE; d += 5) { ctx.moveTo(x + d - SIZE, y + SIZE); ctx.lineTo(x + d + SIZE, y - SIZE) }
      ctx.strokeStyle = water ? '#2c6fa355' : '#4a4f5c66'; ctx.lineWidth = 1; ctx.stroke()
      ctx.restore()
      continue
    }
    const hs = HEIGHT_STYLE[t.height]
    ctx.fillStyle = hs.fill; ctx.fill()
    if (heightMode && detail >= 1) {
      ctx.font = '700 14px ui-monospace, monospace'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
      outlinedText(ctx, hs.label, x, y, hs.tag)
    }
  }
}

/** Link slots shown above a building token: Farm sockets, Village farm support, Wall links. */
export function buildingPips(c: Cell, rules: Rules): Pips | undefined {
  const linked = c.links.filter((l) => l >= 0).length
  switch (c.building) {
    case 1: return rules.farm.adjacent_boost ? undefined : { filled: linked, total: rules.farm.sockets, color: BUILDING_STYLE[0].color }
    case 2: return rules.farm.adjacent_boost
      ? (linked > 0 ? { filled: linked, total: linked, color: BUILDING_STYLE[0].color } : undefined)
      : { filled: c.links[0] >= 0 ? 1 : 0, total: 1, color: BUILDING_STYLE[0].color }
    case 4: return { filled: linked, total: rules.wall.max_links, color: BUILDING_STYLE[3].color }
    default: return undefined
  }
}

export interface Relationship { hexes: Set<number>; alternatives: Set<number>; discounted: Set<number> }

/** Everything the selected building is related to, for highlighting. */
export function relationshipOf(rep: BoardRep, cells: Map<number, Cell>, sel: number | null, canSee: (i: number) => boolean): Relationship {
  const out: Relationship = { hexes: new Set(), alternatives: new Set(), discounted: new Set() }
  const c = sel !== null ? cells.get(sel) : undefined
  if (sel === null || !c || c.building === 0 || !canSee(sel)) return out
  out.hexes.add(sel)
  const at = hexAt(rep.radius, sel)
  const mine = (i: number) => cells.get(i)?.owner === c.owner
  const neighbors = HEX_DIRS.map((d) => addHex(at, d)).filter((n) => onBoard(rep.radius, n)).map((n) => hexIndex(rep.radius, n))
  if (c.building === 1) {
    c.links.filter((l) => l >= 0).forEach((l) => out.hexes.add(l))
    neighbors.forEach((n) => { const v = cells.get(n); if (v && v.building === 2 && mine(n) && v.links[0] < 0) out.alternatives.add(n) })
  } else if (c.building === 2) {
    const farm = c.links[0]
    if (farm >= 0) {
      out.hexes.add(farm)
      cells.get(farm)!.links.filter((l) => l >= 0).forEach((l) => out.hexes.add(l))
    } else {
      neighbors.forEach((n) => { const v = cells.get(n); if (v && v.building === 1 && mine(n) && v.links.some((l) => l < 0)) out.alternatives.add(n) })
    }
  } else if (c.building === 4) {
    c.links.filter((l) => l >= 0).forEach((l) => out.hexes.add(l))
    neighbors.forEach((n) => {
      const v = cells.get(n)
      if (v && v.building === 4 && mine(n) && !c.links.includes(n)) out.alternatives.add(n)
    })
  } else if (c.building === 3) {
    neighbors.forEach((n) => { if (rep.terrain[n]?.kind === 'open') out.discounted.add(n) })
  }
  return out
}

/** The next action: an arrow for a move, a dashed ring for a gather target, a ring for a build. */
function drawPlan(ctx: CanvasRenderingContext2D, rep: BoardRep, geo: Geometry, a: PlayerAction, color: string, s: number, label?: string) {
  const pos = (h: Hex) => { const i = hexIndex(rep.radius, h); return { x: geo.x[i], y: geo.y[i] } }
  ctx.lineCap = 'round'; ctx.lineJoin = 'round'
  switch (a.kind) {
    case 'move': {
      const p = pos(a.from), q = pos(a.to)
      const ang = Math.atan2(q.y - p.y, q.x - p.x)
      const tip = { x: q.x - Math.cos(ang) * s * 0.35, y: q.y - Math.sin(ang) * s * 0.35 }
      const head = (w: number) => {
        ctx.beginPath()
        ctx.moveTo(tip.x, tip.y)
        ctx.lineTo(tip.x - Math.cos(ang - 0.5) * w, tip.y - Math.sin(ang - 0.5) * w)
        ctx.lineTo(tip.x - Math.cos(ang + 0.5) * w, tip.y - Math.sin(ang + 0.5) * w)
        ctx.closePath()
      }
      for (const [stroke, width] of [['rgba(6,7,10,0.9)', 8], [color, 4.5]] as const) {
        ctx.strokeStyle = stroke; ctx.lineWidth = width
        ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(tip.x, tip.y); ctx.stroke()
        head(width === 8 ? 13 : 10.5); ctx.fillStyle = stroke; ctx.fill()
        if (width === 8) ctx.stroke()
      }
      ctx.font = '700 10px Geist Variable, ui-sans-serif, sans-serif'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
      outlinedText(ctx, label ?? String(a.troops), (p.x + q.x) / 2, (p.y + q.y) / 2 - 9, '#ffffff')
      return
    }
    case 'gather': {
      const p = pos(a.target)
      hexPath(ctx, p.x, p.y, s + 3); ctx.setLineDash([5, 3]); ctx.strokeStyle = color; ctx.lineWidth = 3; ctx.stroke(); ctx.setLineDash([])
      return
    }
    case 'build': {
      const p = pos(a.at)
      hexPath(ctx, p.x, p.y, s + 3); ctx.strokeStyle = 'rgba(6,7,10,0.9)'; ctx.lineWidth = 6; ctx.stroke()
      ctx.strokeStyle = color; ctx.lineWidth = 3; ctx.stroke()
      return
    }
    case 'demolish': {
      const p = pos(a.at), r = s * 0.45
      for (const [stroke, width] of [['rgba(6,7,10,0.9)', 6], [color, 3]] as const) {
        ctx.strokeStyle = stroke; ctx.lineWidth = width
        ctx.beginPath(); ctx.moveTo(p.x - r, p.y - r); ctx.lineTo(p.x + r, p.y + r); ctx.moveTo(p.x + r, p.y - r); ctx.lineTo(p.x - r, p.y + r); ctx.stroke()
      }
      return
    }
    case 'rejected':
    case 'pass':
      return
    default: { const never: never = a; return never }
  }
}

interface Props {
  rep: BoardRep
  cells: Map<number, Cell>
  perspective: number
  visible: Set<number> | null
  selected: number | null
  heightMode: boolean
  tick: number
  /** The followed player's next action, drawn as an arrow or a ring. */
  plan?: { player: number; action: PlayerAction } | null
  /** Several upcoming actions, the first drawn boldest. */
  plans?: { player: number; action: PlayerAction; label?: string }[]
  /** Place names drawn over the map. */
  labels?: { idx: number; name: string }[]
  /** Draw water as open sea rather than hatched impassable hexes. */
  calmSea?: boolean
  /** Centre the camera on this hex at zoom `k`; a new object re-centres. */
  focus?: { idx: number; k: number } | null
  /** `alt` is a right-click or a shift-click. */
  onSelect: (idx: number | null, mods?: { alt: boolean }) => void
}

/** How long a newly built hex keeps its highlight ring. */
const NEW_BUILD_TICKS = 40

export function Board({ rep, cells, perspective, visible, selected, heightMode, tick, plan, plans, labels, calmSea = false, focus, onSelect }: Props) {
  const wrap = useRef<HTMLDivElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const mini = useRef<HTMLCanvasElement>(null)
  const [cam, setCam] = useState({ x: 0, y: 0, k: 1 })
  const [dims, setDims] = useState({ w: 800, h: 600 })
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null)
  const all = useMemo(() => Array.from({ length: (2 * rep.radius + 1) ** 2 }, (_, i) => i).filter((i) => rep.terrain[i]), [rep])
  const recentBuilds = useMemo(() => {
    const out: { idx: number; kind: number; age: number }[] = []
    for (let t = tick; t >= Math.max(0, tick - NEW_BUILD_TICKS + 1); t--)
      for (const e of rep.events[t] ?? [])
        if (e.type === 'built') out.push({ idx: hexIndex(rep.radius, e.at), kind: BUILDING_KEYS.indexOf(e.building as typeof BUILDING_KEYS[number]) + 1, age: tick - t })
    return out
  }, [rep, tick])
  const fit = (w: number, h: number) => Math.min(w, h) / (SIZE * Math.sqrt(3) * (2 * rep.radius + 2))

  useEffect(() => {
    const el = wrap.current!
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth, h = el.clientHeight
      setDims({ w, h })
      const zoom = Number(new URLSearchParams(location.search).get('zoom')) || 1
      setCam((c) => (c.k === 1 && c.x === 0 && c.y === 0 ? { x: 0, y: 0, k: fit(w, h) * zoom } : c))
    })
    ro.observe(el)
    return () => ro.disconnect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rep.radius])

  const fogged = (idx: number) => visible !== null && !visible.has(idx)
  const rel = useMemo(() => relationshipOf(rep, cells, selected, (i) => !fogged(i)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rep, cells, selected, visible])

  const geo = useMemo(() => geometry(rep, all), [rep, all])
  useEffect(() => {
    if (focus) setCam({ x: -geo.x[focus.idx] * focus.k, y: -geo.y[focus.idx] * focus.k, k: focus.k })
  }, [focus, geo])

  const floor = useMemo(() => stormFloor(rep), [rep])
  const storm = stormRadius(rep.rules, tick, rep.radius, floor)
  const terrainLayer = useRef<{ key: string; canvas: HTMLCanvasElement } | null>(null)

  useEffect(() => {
    const c = canvas.current!
    const dpr = window.devicePixelRatio || 1
    if (c.width !== dims.w * dpr || c.height !== dims.h * dpr) { c.width = dims.w * dpr; c.height = dims.h * dpr }
    const ctx = c.getContext('2d')!
    const px = cam.k * SIZE
    const detail = px >= 13 ? 2 : px >= 7 ? 1 : 0
    const s = SIZE - 1.7
    const world = () => { ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.translate(dims.w / 2 + cam.x, dims.h / 2 + cam.y); ctx.scale(cam.k, cam.k) }

    // Terrain never changes during a match, so it is drawn once per camera position.
    const key = `${dims.w}x${dims.h}@${dpr}:${cam.x},${cam.y},${cam.k}:${heightMode}:${calmSea}`
    if (terrainLayer.current?.key !== key) {
      const layer = terrainLayer.current?.canvas ?? document.createElement('canvas')
      layer.width = c.width; layer.height = c.height
      const t = layer.getContext('2d')!
      t.setTransform(dpr, 0, 0, dpr, 0, 0); t.translate(dims.w / 2 + cam.x, dims.h / 2 + cam.y); t.scale(cam.k, cam.k)
      drawTerrain(t, rep, geo, s, detail, heightMode, calmSea)
      terrainLayer.current = { key, canvas: layer }
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.clearRect(0, 0, c.width, c.height)
    ctx.drawImage(terrainLayer.current.canvas, 0, 0)
    world()

    // Hexes on screen (plus a margin); everything per-hex below is skipped off screen.
    const vx = (-dims.w / 2 - cam.x) / cam.k - SIZE, vy = (-dims.h / 2 - cam.y) / cam.k - SIZE
    const vw = dims.w / cam.k + 2 * SIZE, vh = dims.h / cam.k + 2 * SIZE
    const onScreen = (i: number) => { const x = geo.x[i], y = geo.y[i]; return x >= vx && x <= vx + vw && y >= vy && y <= vy + vh }
    const fogged = (idx: number) => visible !== null && !visible.has(idx)
    const seen = (i: number) => !fogged(i)
    const emphasized = (i: number) => rel.hexes.has(i)
    const ownerAt = (i: number) => { if (i < 0 || fogged(i)) return -1; return cells.get(i)?.owner ?? -1 }

    if (visible !== null) {
      ctx.beginPath()
      for (const idx of all) if (rep.terrain[idx]!.kind === 'open' && !visible.has(idx)) hexPoly(ctx, geo.x[idx], geo.y[idx], s)
      ctx.fillStyle = 'rgba(6,7,10,0.62)'; ctx.fill()
    }

    if (storm !== null) {
      ctx.beginPath()
      for (const idx of all) if (hexDist(hexAt(rep.radius, idx), { q: 0, r: 0 }) > storm) hexPoly(ctx, geo.x[idx], geo.y[idx], s)
      ctx.fillStyle = 'rgba(168,85,247,0.28)'; ctx.fill()
    }

    // Territory: one tinted fill, one border path and one front path per player.
    const players = rep.names.length
    const fills = Array.from({ length: players }, () => new Path2D())
    const borders = Array.from({ length: players }, () => new Path2D())
    const fronts = Array.from({ length: players }, () => new Path2D())
    const inset = s - 1.4
    for (const [idx, cell] of cells) {
      if (cell.owner < 0 || fogged(idx)) continue
      const o = cell.owner, x = geo.x[idx], y = geo.y[idx]
      hexPoly(fills[o], x, y, inset)
      for (let d = 0; d < 6; d++) {
        const n = geo.nb[idx * 6 + d]
        const other = ownerAt(n)
        if (other === o) continue
        const [a, b] = EDGE[d]
        const path = other >= 0 ? fronts[o] : borders[o]
        path.moveTo(x + inset * CORNER[a][0], y + inset * CORNER[a][1])
        path.lineTo(x + inset * CORNER[b][0], y + inset * CORNER[b][1])
      }
    }
    ctx.lineCap = 'round'; ctx.lineJoin = 'round'
    for (let o = 0; o < players; o++) {
      const col = PLAYER_COLORS[o % 8]
      ctx.fillStyle = col + (detail === 0 ? '48' : '30'); ctx.fill(fills[o])
      if (detail >= 1) {
        ctx.globalAlpha = 0.28; ctx.strokeStyle = col; ctx.lineWidth = 6
        ctx.stroke(borders[o]); ctx.stroke(fronts[o])
        ctx.globalAlpha = 1
      }
      ctx.strokeStyle = col
      ctx.lineWidth = 2.4; ctx.stroke(borders[o])
      ctx.lineWidth = 3.6; ctx.stroke(fronts[o])
    }

    if (detail >= 2) {
      ctx.font = '7px sans-serif'; ctx.textAlign = 'left'; ctx.textBaseline = 'top'
      for (const [idx, cell] of cells) {
        if (cell.owner < 0 || fogged(idx) || !onScreen(idx)) continue
        ctx.fillStyle = PLAYER_COLORS[cell.owner % 8]
        ctx.fillText(PLAYER_MARKS[cell.owner % 8], geo.x[idx] - SIZE * 0.78, geo.y[idx] - SIZE * 0.5)
      }
      if (!heightMode) {
        ctx.font = '600 10.5px Geist Variable, ui-sans-serif, sans-serif'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
        ctx.fillStyle = 'rgba(255,255,255,0.88)'
        for (const idx of all) {
          const t = rep.terrain[idx]!
          if (t.kind !== 'open' || fogged(idx) || !onScreen(idx)) continue
          const cell = cells.get(idx)
          if (cell && (cell.owner >= 0 || cell.building > 0)) continue
          ctx.fillText(String(cell ? cell.troops : rep.neutral[idx]), geo.x[idx], t.height === 0 ? geo.y[idx] + 1 : geo.y[idx] - SIZE * 0.08)
        }
      }
    }

    for (const [idx, cell] of cells) {
      if (cell.building === 0 || !seen(idx) || !onScreen(idx)) continue
      const x = geo.x[idx], y = geo.y[idx]
      const ps = detail === 0 ? s * 0.9 : s * 0.8
      drawPlot(ctx, cell.building, x, y, ps, () => hexPath(ctx, x, y, ps), idx)
    }

    for (const b of recentBuilds) {
      if (!seen(b.idx) || cells.get(b.idx)?.building !== b.kind || !onScreen(b.idx)) continue
      const fade = 1 - b.age / NEW_BUILD_TICKS
      const col = BUILDING_STYLE[b.kind - 1].color
      ctx.globalAlpha = 0.25 * fade
      hexPath(ctx, geo.x[b.idx], geo.y[b.idx], s + 4 + 3 * (1 - fade)); ctx.strokeStyle = col; ctx.lineWidth = 6; ctx.stroke()
      ctx.globalAlpha = 0.35 + 0.65 * fade
      hexPath(ctx, geo.x[b.idx], geo.y[b.idx], s + 1.5 + 3 * (1 - fade)); ctx.lineWidth = 2.6; ctx.stroke()
      ctx.globalAlpha = 1
    }

    if (detail >= 2) {
      for (const [idx, cell] of cells) {
        if (!seen(idx) || cell.building === 0) continue
        const a = { x: geo.x[idx], y: geo.y[idx] }
        if (cell.building === 1) {
          for (const l of cell.links) {
            if (l < 0 || !seen(l) || (!onScreen(idx) && !onScreen(l))) continue
            const b = { x: geo.x[l], y: geo.y[l] }
            const hot = emphasized(idx) && emphasized(l)
            ctx.lineCap = 'round'
            ctx.strokeStyle = 'rgba(6,7,10,0.9)'; ctx.lineWidth = hot ? 7 : 5.5
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
            ctx.strokeStyle = hot ? '#d6ffc8' : '#86e36a'; ctx.lineWidth = hot ? 4.4 : 3
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
          }
        } else if (cell.building === 4) {
          for (const l of cell.links) {
            if (l < 0 || l < idx || !seen(l) || (!onScreen(idx) && !onScreen(l))) continue
            const b = { x: geo.x[l], y: geo.y[l] }
            const hot = emphasized(idx) && emphasized(l)
            ctx.lineCap = 'round'
            ctx.strokeStyle = 'rgba(6,7,10,0.9)'; ctx.lineWidth = hot ? 10 : 8.5
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
            ctx.strokeStyle = hot ? '#e6e9ef' : '#9aa1ae'; ctx.lineWidth = hot ? 6.5 : 5
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
            ctx.strokeStyle = hot ? '#ffffff' : '#d5d9e1'; ctx.lineWidth = 1.4
            ctx.beginPath(); ctx.moveTo(a.x, a.y - 1.2); ctx.lineTo(b.x, b.y - 1.2); ctx.stroke()
          }
        }
      }
    }

    const rules = rep.rules
    if (detail >= 1) {
      for (const [idx, cell] of cells) {
        if (!seen(idx) || !onScreen(idx)) continue
        const x = geo.x[idx], y = geo.y[idx]
        const hasB = cell.building > 0
        const troops = cell.troops
        const flat = rep.terrain[idx]?.height === 0
        if (!heightMode && detail === 2 && troops > 0 && (cell.owner >= 0 || hasB)) {
          ctx.font = `600 ${hasB ? 9.5 : 10.5}px Geist Variable, ui-sans-serif, sans-serif`; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
          outlinedText(ctx, String(troops), x, hasB ? y + SIZE * 0.36 : flat ? y + 1 : y - SIZE * 0.08, '#ffffff')
        }
        if (!hasB) continue
        if (detail < 2) {
          drawBuilding(ctx, cell.building, x, y, SIZE * 0.5)
          continue
        }
        drawBuilding(ctx, cell.building, x, y - SIZE * 0.22, SIZE * 0.38, { pips: buildingPips(cell, rules) })
      }
    }

    if (detail >= 2 && !heightMode) {
      for (const idx of all) {
        const t = rep.terrain[idx]!
        if (t.kind !== 'open' || t.height === 0 || fogged(idx) || !onScreen(idx)) continue
        const hasB = (cells.get(idx)?.building ?? 0) > 0
        drawHeightMark(ctx, t.height, geo.x[idx], geo.y[idx] + SIZE * (hasB ? 0.7 : 0.54), SIZE * 0.25)
      }
    }

    for (const i of rel.discounted) {
      const x = geo.x[i], y = geo.y[i]
      hexPath(ctx, x, y, s); ctx.fillStyle = 'rgba(111,214,255,0.22)'; ctx.fill()
      ctx.setLineDash([3, 2]); ctx.strokeStyle = '#6fd6ff'; ctx.lineWidth = 1.6; ctx.stroke(); ctx.setLineDash([])
      if (detail >= 1) { ctx.font = '700 7px sans-serif'; ctx.textAlign = 'center'; outlinedText(ctx, `-${rep.rules.quarry.troop_discount_pct}%`, x, y - SIZE * 0.5, '#9fe6ff') }
    }
    for (const i of rel.alternatives) {
      hexPath(ctx, geo.x[i], geo.y[i], s - 0.5); ctx.setLineDash([4, 3]); ctx.strokeStyle = '#ffd36b'; ctx.lineWidth = 2; ctx.stroke(); ctx.setLineDash([])
    }
    for (const i of rel.hexes) {
      hexPath(ctx, geo.x[i], geo.y[i], s + 1); ctx.strokeStyle = i === selected ? '#fff' : '#e8f5ff'; ctx.lineWidth = i === selected ? 3.2 : 2.2; ctx.stroke()
    }
    if (selected !== null && !rel.hexes.has(selected)) {
      hexPath(ctx, geo.x[selected], geo.y[selected], s + 1); ctx.strokeStyle = '#fff'; ctx.lineWidth = 3; ctx.stroke()
    }
    if (plan) drawPlan(ctx, rep, geo, plan.action, PLAYER_COLORS[plan.player % 8], s)
    if (plans) {
      for (let k = plans.length - 1; k >= 0; k--) {
        ctx.globalAlpha = k === 0 ? 1 : Math.max(0.3, 0.75 - k * 0.07)
        drawPlan(ctx, rep, geo, plans[k].action, PLAYER_COLORS[plans[k].player % 8], s, plans[k].label)
      }
      ctx.globalAlpha = 1
    }
    if (labels) {
      ctx.font = `600 ${Math.max(9, Math.min(13, 11 / Math.sqrt(cam.k)))}px Geist Variable, ui-sans-serif, sans-serif`
      ctx.textAlign = 'center'; ctx.textBaseline = 'bottom'
      for (const l of labels) {
        if (!onScreen(l.idx)) continue
        ctx.globalAlpha = 0.85
        outlinedText(ctx, l.name, geo.x[l.idx], geo.y[l.idx] - SIZE * 0.62, '#f3efe3')
      }
      ctx.globalAlpha = 1
    }
  }, [rep, cells, visible, cam, dims, selected, all, perspective, heightMode, rel, recentBuilds, geo, storm, plan, plans, labels, calmSea])

  useEffect(() => {
    const c = mini.current!
    const ctx = c.getContext('2d')!
    const W = c.width, ext = SIZE * Math.sqrt(3) * (rep.radius + 1)
    const k = W / (2 * ext)
    ctx.clearRect(0, 0, W, W)
    ctx.fillStyle = '#0e0e10'; ctx.fillRect(0, 0, W, W)
    for (const idx of all) {
      const t = rep.terrain[idx]!
      const { x, y } = toPx(hexAt(rep.radius, idx))
      const cell = cells.get(idx)
      const hidden = fogged(idx)
      ctx.fillStyle = t.kind !== 'open' ? '#262a33' : hidden ? '#14171d' : cell && cell.owner >= 0 ? PLAYER_COLORS[cell.owner % 8] : HEIGHT_STYLE[t.height].fill
      ctx.fillRect(W / 2 + x * k - 1.5, W / 2 + y * k - 1.5, 3, 3)
    }
    ctx.strokeStyle = '#fff'; ctx.lineWidth = 1
    const vw = (dims.w / cam.k) * k, vh = (dims.h / cam.k) * k
    ctx.strokeRect(W / 2 + (-cam.x / cam.k) * k - vw / 2, W / 2 + (-cam.y / cam.k) * k - vh / 2, vw, vh)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rep, cells, visible, cam, dims, all])

  const pick = (ev: React.PointerEvent) => {
    const mods = { alt: ev.shiftKey || ev.button === 2 }
    const r = canvas.current!.getBoundingClientRect()
    const wx = (ev.clientX - r.left - dims.w / 2 - cam.x) / cam.k
    const wy = (ev.clientY - r.top - dims.h / 2 - cam.y) / cam.k
    const rf = wy / (SIZE * 1.5)
    const qf = wx / (SIZE * Math.sqrt(3)) - rf / 2
    let q = Math.round(qf), rr = Math.round(rf)
    const s = Math.round(-qf - rf)
    const dq = Math.abs(q - qf), dr = Math.abs(rr - rf), ds = Math.abs(s + qf + rf)
    if (dq > dr && dq > ds) q = -rr - s; else if (dr > ds) rr = -q - s
    const h = { q, r: rr }
    onSelect(onBoard(rep.radius, h) && rep.terrain[hexIndex(rep.radius, h)] ? hexIndex(rep.radius, h) : null, mods)
  }

  return (
    <div ref={wrap} className="relative h-full w-full overflow-hidden rounded-xl border bg-card">
      <canvas
        ref={canvas}
        style={{ width: dims.w, height: dims.h, touchAction: 'none', cursor: 'grab' }}
        onWheel={(e) => setCam((c) => ({ ...c, k: Math.min(7, Math.max(0.25, c.k * Math.exp(-e.deltaY * 0.0015))) }))}
        onPointerDown={(e) => { drag.current = { x: e.clientX, y: e.clientY, moved: false }; e.currentTarget.setPointerCapture(e.pointerId) }}
        onPointerMove={(e) => {
          const d = drag.current
          if (!d) return
          const dx = e.clientX - d.x, dy = e.clientY - d.y
          if (Math.abs(dx) + Math.abs(dy) > 3) d.moved = true
          d.x = e.clientX; d.y = e.clientY
          setCam((c) => ({ ...c, x: c.x + dx, y: c.y + dy }))
        }}
        onPointerUp={(e) => { if (drag.current && !drag.current.moved) pick(e); drag.current = null }}
        onContextMenu={(e) => e.preventDefault()}
      />
      <div className="pointer-events-none absolute left-4 top-3.5 text-[11px] font-semibold tracking-[0.3em] text-muted-foreground">HEXARENA</div>
      <Badge variant="outline" className="absolute right-3 top-3 gap-1.5 bg-background/80 backdrop-blur">
        {perspective < 0 ? <Eye className="size-3" /> : <EyeOff className="size-3" />}
        {perspective < 0 ? 'God view, no fog' : `${rep.names[perspective]} fog`}
      </Badge>
      {storm !== null && (
        <Badge variant="outline" className="absolute right-3 top-11 border-purple-400/50 bg-background/80 text-purple-200 backdrop-blur">
          Storm: safe within {storm} of the centre
        </Badge>
      )}
      <canvas ref={mini} width={140} height={140} className="absolute bottom-3 right-3 rounded-lg border bg-background/80" style={{ width: 112, height: 112 }} />
      <div className="absolute bottom-3 left-3 flex gap-1">
        <Button variant="outline" size="icon-sm" onClick={() => setCam((c) => ({ ...c, k: Math.min(7, c.k * 1.25) }))}><ZoomIn /></Button>
        <Button variant="outline" size="icon-sm" onClick={() => setCam((c) => ({ ...c, k: Math.max(0.25, c.k / 1.25) }))}><ZoomOut /></Button>
        <Button variant="outline" size="icon-sm" onClick={() => setCam({ x: 0, y: 0, k: fit(dims.w, dims.h) })}><Maximize2 /></Button>
      </div>
    </div>
  )
}

