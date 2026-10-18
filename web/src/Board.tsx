import { useEffect, useMemo, useRef, useState } from 'react'
import type { Cell, Hex, ViewerReplay } from './model'
import { HEIGHT_STYLE, HEX_DIRS, PLAYER_COLORS, PLAYER_MARKS, addHex, hexAt, hexIndex, onBoard } from './model'
import { BUILDING_STYLE, drawBuilding } from './icons'

const SIZE = 22
const toPx = (h: Hex) => ({ x: SIZE * Math.sqrt(3) * (h.q + h.r / 2), y: SIZE * 1.5 * h.r })

function hexPath(ctx: CanvasRenderingContext2D, x: number, y: number, s: number) {
  ctx.beginPath()
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 180) * (60 * i - 30)
    const px = x + s * Math.cos(a), py = y + s * Math.sin(a)
    if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py)
  }
  ctx.closePath()
}

function outlinedText(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, fill: string) {
  ctx.lineWidth = 2.2; ctx.strokeStyle = 'rgba(6,7,10,0.9)'; ctx.strokeText(text, x, y)
  ctx.fillStyle = fill; ctx.fillText(text, x, y)
}

export interface Relationship { hexes: Set<number>; alternatives: Set<number>; discounted: Set<number> }

/** Everything the selected building is related to, for highlighting. */
export function relationshipOf(rep: ViewerReplay, cells: Map<number, Cell>, sel: number | null, canSee: (i: number) => boolean): Relationship {
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

interface Props {
  rep: ViewerReplay
  cells: Map<number, Cell>
  perspective: number
  visible: Set<number> | null
  selected: number | null
  heightMode: boolean
  onSelect: (idx: number | null) => void
}

export function Board({ rep, cells, perspective, visible, selected, heightMode, onSelect }: Props) {
  const wrap = useRef<HTMLDivElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const mini = useRef<HTMLCanvasElement>(null)
  const [cam, setCam] = useState({ x: 0, y: 0, k: 1 })
  const [dims, setDims] = useState({ w: 800, h: 600 })
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null)
  const all = useMemo(() => Array.from({ length: (2 * rep.radius + 1) ** 2 }, (_, i) => i).filter((i) => rep.terrain[i]), [rep])
  const fit = (w: number, h: number) => Math.min(w, h) / (SIZE * Math.sqrt(3) * (2 * rep.radius + 2))

  useEffect(() => {
    const el = wrap.current!
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth, h = el.clientHeight
      setDims({ w, h })
      setCam((c) => (c.k === 1 && c.x === 0 && c.y === 0 ? { x: 0, y: 0, k: fit(w, h) } : c))
    })
    ro.observe(el)
    return () => ro.disconnect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rep.radius])

  const fogged = (idx: number) => visible !== null && !visible.has(idx)
  const rel = useMemo(() => relationshipOf(rep, cells, selected, (i) => !fogged(i)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rep, cells, selected, visible])

  useEffect(() => {
    const c = canvas.current!
    const dpr = window.devicePixelRatio || 1
    c.width = dims.w * dpr; c.height = dims.h * dpr
    const ctx = c.getContext('2d')!
    ctx.scale(dpr, dpr)
    ctx.clearRect(0, 0, dims.w, dims.h)
    ctx.translate(dims.w / 2 + cam.x, dims.h / 2 + cam.y)
    ctx.scale(cam.k, cam.k)
    const px = cam.k * SIZE
    const detail = px >= 13 ? 2 : px >= 7 ? 1 : 0
    const s = SIZE - 0.8
    const center = (i: number) => toPx(hexAt(rep.radius, i))

    for (const idx of all) {
      const t = rep.terrain[idx]!
      const h = hexAt(rep.radius, idx)
      const { x, y } = toPx(h)
      hexPath(ctx, x, y, s)
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
      const hidden = fogged(idx)
      if (hidden) { hexPath(ctx, x, y, s); ctx.fillStyle = 'rgba(6,7,10,0.62)'; ctx.fill() }
      hexPath(ctx, x, y, s)
      ctx.strokeStyle = '#0a0c10'; ctx.lineWidth = 1.2; ctx.lineJoin = 'round'; ctx.stroke()
      const cell = cells.get(idx)
      if (!hidden && cell && cell.owner >= 0) {
        hexPath(ctx, x, y, s - 1.5)
        ctx.strokeStyle = PLAYER_COLORS[cell.owner % 8]; ctx.lineWidth = 2.2; ctx.stroke()
        ctx.fillStyle = PLAYER_COLORS[cell.owner % 8] + '1a'; ctx.fill()
        if (detail >= 2) {
          ctx.fillStyle = PLAYER_COLORS[cell.owner % 8]
          ctx.font = '7px sans-serif'; ctx.textAlign = 'left'; ctx.textBaseline = 'top'
          ctx.fillText(PLAYER_MARKS[cell.owner % 8], x - SIZE * 0.78, y - SIZE * 0.5)
        }
      }
      if (heightMode && detail >= 1) {
        ctx.fillStyle = hs.tag
        ctx.font = '700 14px ui-monospace, monospace'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
        outlinedText(ctx, hs.label, x, y, hs.tag)
      }
    }

    const seen = (i: number) => !fogged(i)
    const emphasized = (i: number) => rel.hexes.has(i)

    if (detail >= 2) {
      for (const [idx, cell] of cells) {
        if (!seen(idx) || cell.building === 0) continue
        const a = center(idx)
        if (cell.building === 1) {
          for (const l of cell.links) {
            if (l < 0 || !seen(l)) continue
            const b = center(l)
            const hot = emphasized(idx) && emphasized(l)
            ctx.lineCap = 'round'
            ctx.strokeStyle = 'rgba(6,7,10,0.9)'; ctx.lineWidth = hot ? 7 : 5
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
            ctx.strokeStyle = hot ? '#d6ffc8' : '#8be36f'; ctx.lineWidth = hot ? 4.4 : 2.8
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
          }
        } else if (cell.building === 4) {
          for (const l of cell.links) {
            if (l < 0 || l < idx || !seen(l)) continue
            const b = center(l)
            const hot = emphasized(idx) && emphasized(l)
            ctx.lineCap = 'butt'
            ctx.strokeStyle = '#0a0c10'; ctx.lineWidth = hot ? 11 : 9
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
            ctx.strokeStyle = hot ? '#ffffff' : '#c9ced8'; ctx.lineWidth = hot ? 7 : 5.5
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
            ctx.setLineDash([3, 3]); ctx.strokeStyle = '#7b8190'; ctx.lineWidth = 1
            ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.setLineDash([])
          }
        }
      }
    }

    const rules = rep.rules
    for (const [idx, cell] of cells) {
      if (!seen(idx)) continue
      const { x, y } = center(idx)
      const hasB = cell.building > 0
      const troops = cell.troops
      if (detail === 0) {
        if (hasB) {
          ctx.fillStyle = BUILDING_STYLE[cell.building - 1].color
          ctx.beginPath(); ctx.arc(x, y, SIZE * 0.34, 0, Math.PI * 2); ctx.fill()
        }
        continue
      }
      if (!heightMode && detail === 2 && troops > 0) {
        ctx.font = '600 10px ui-monospace, monospace'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
        outlinedText(ctx, String(troops), x, hasB ? y + SIZE * 0.34 : y + 1, cell.owner >= 0 ? '#ffffff' : '#cfd3da')
      }
      if (!hasB) continue
      const supported = cell.building === 2 && cell.links[0] >= 0
      const wallLinks = cell.building === 4 ? cell.links.filter((l) => l >= 0).length : 0
      drawBuilding(ctx, cell.building, x, y - SIZE * 0.3, SIZE * 0.32, {
        dashed: (cell.building === 2 && !supported) || (cell.building === 4 && wallLinks === 0),
        doubleRing: supported || (cell.building === 4 && wallLinks === 2),
      })
      if (detail < 2) continue
      ctx.font = '700 7.5px sans-serif'; ctx.textAlign = 'left'; ctx.textBaseline = 'middle'
      const lx = x + SIZE * 0.42, ly = y - SIZE * 0.36
      if (cell.building === 1) {
        const n = cell.links.filter((l) => l >= 0).length
        outlinedText(ctx, `${n}/${rules.farm.sockets}`, lx, ly, '#8be36f')
        for (let i = 0; i < rules.farm.sockets; i++) {
          ctx.beginPath(); ctx.arc(x + (i - 1) * SIZE * 0.2, y - SIZE * 0.78, SIZE * 0.07, 0, Math.PI * 2)
          ctx.fillStyle = i < n ? '#8be36f' : 'rgba(0,0,0,0)'; ctx.fill()
          ctx.strokeStyle = '#8be36f'; ctx.lineWidth = 0.9; ctx.stroke()
        }
      } else if (cell.building === 2) {
        const inc = rules.base_income + rules.village.bonus + (supported ? rules.village.farm_bonus : 0)
        outlinedText(ctx, `${inc}/tick`, lx, ly, supported ? '#ffd9a1' : '#c9a373')
        if (supported) drawBuilding(ctx, 1, x - SIZE * 0.5, y - SIZE * 0.62, SIZE * 0.17)
      } else if (cell.building === 4) {
        const mult = rules.wall.defense_milli_by_links[wallLinks] / 1000
        outlinedText(ctx, `${wallLinks}/${rules.wall.max_links} ${mult.toFixed(1)}x`, lx - SIZE * 0.1, ly, '#f1f3f7')
      }
    }

    if (detail >= 1) {
      for (const idx of all) {
        const t = rep.terrain[idx]!
        if (t.kind !== 'open' || heightMode || fogged(idx)) continue
        const cell = cells.get(idx)
        if (detail < 2) continue
        const { x, y } = center(idx)
        const hs = HEIGHT_STYLE[t.height]
        ctx.font = '600 7px ui-sans-serif, system-ui, sans-serif'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
        ctx.globalAlpha = 0.7
        ctx.fillStyle = hs.tag
        ctx.fillText(hs.label, x, y + SIZE * (cell && cell.building > 0 ? 0.74 : 0.66))
        ctx.globalAlpha = 1
      }
    }

    for (const i of rel.discounted) {
      const { x, y } = center(i)
      hexPath(ctx, x, y, s); ctx.fillStyle = 'rgba(111,214,255,0.22)'; ctx.fill()
      ctx.setLineDash([3, 2]); ctx.strokeStyle = '#6fd6ff'; ctx.lineWidth = 1.6; ctx.stroke(); ctx.setLineDash([])
      if (detail >= 1) { ctx.font = '700 7px sans-serif'; ctx.textAlign = 'center'; outlinedText(ctx, '-25%', x, y - SIZE * 0.5, '#9fe6ff') }
    }
    for (const i of rel.alternatives) {
      const { x, y } = center(i)
      hexPath(ctx, x, y, s - 0.5); ctx.setLineDash([4, 3]); ctx.strokeStyle = '#ffd36b'; ctx.lineWidth = 2; ctx.stroke(); ctx.setLineDash([])
    }
    for (const i of rel.hexes) {
      const { x, y } = center(i)
      hexPath(ctx, x, y, s + 1); ctx.strokeStyle = i === selected ? '#fff' : '#e8f5ff'; ctx.lineWidth = i === selected ? 3.2 : 2.2; ctx.stroke()
    }
    if (selected !== null && !rel.hexes.has(selected)) {
      const { x, y } = center(selected)
      hexPath(ctx, x, y, s + 1); ctx.strokeStyle = '#fff'; ctx.lineWidth = 3; ctx.stroke()
    }
  }, [rep, cells, visible, cam, dims, selected, all, perspective, heightMode, rel])

  useEffect(() => {
    const c = mini.current!
    const ctx = c.getContext('2d')!
    const W = c.width, ext = SIZE * Math.sqrt(3) * (rep.radius + 1)
    const k = W / (2 * ext)
    ctx.clearRect(0, 0, W, W)
    ctx.fillStyle = '#0b0d11'; ctx.fillRect(0, 0, W, W)
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
    onSelect(onBoard(rep.radius, h) && rep.terrain[hexIndex(rep.radius, h)] ? hexIndex(rep.radius, h) : null)
  }

  return (
    <div ref={wrap} className="relative h-full w-full overflow-hidden rounded-lg bg-[#0a0c10]">
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
      />
      <div className="absolute left-3 top-3 text-[11px] tracking-[0.25em] text-neutral-400">HEXARENA</div>
      <div className="absolute right-3 top-3 rounded-md border border-[#252b37] bg-[#101218cc] px-2 py-1 text-[11px]">
        {perspective < 0 ? 'God view, no fog' : `${rep.names[perspective]} fog`}
      </div>
      <canvas ref={mini} width={140} height={140} className="absolute bottom-3 right-3 rounded-md border border-[#252b37]" style={{ width: 120, height: 120 }} />
      <div className="absolute bottom-3 left-3 flex gap-1">
        <button className="btn" onClick={() => setCam((c) => ({ ...c, k: Math.min(7, c.k * 1.25) }))}>+</button>
        <button className="btn" onClick={() => setCam((c) => ({ ...c, k: Math.max(0.25, c.k / 1.25) }))}>-</button>
        <button className="btn" onClick={() => setCam({ x: 0, y: 0, k: fit(dims.w, dims.h) })}>Fit</button>
      </div>
    </div>
  )
}

