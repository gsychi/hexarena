import { useEffect, useMemo, useRef, useState } from 'react'
import type { Cell, Hex, ViewerReplay } from './model'
import { BUILDINGS, PLAYER_COLORS, PLAYER_MARKS, hexAt, hexDist, hexIndex, onBoard } from './model'

const SIZE = 22
const HEIGHT_FILL = ['#1d3b2a', '#2c4a2a', '#4a4f2a', '#5e4a2c', '#6b4a3a']
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

interface Props {
  rep: ViewerReplay
  cells: Map<number, Cell>
  /** Viewer perspective: player id, or -1 for god view. */
  perspective: number
  visible: Set<number> | null
  selected: number | null
  onSelect: (idx: number | null) => void
}

export function Board({ rep, cells, perspective, visible, selected, onSelect }: Props) {
  const wrap = useRef<HTMLDivElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const mini = useRef<HTMLCanvasElement>(null)
  const [cam, setCam] = useState({ x: 0, y: 0, k: 1 })
  const [dims, setDims] = useState({ w: 800, h: 600 })
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null)
  const all = useMemo(() => Array.from({ length: (2 * rep.radius + 1) ** 2 }, (_, i) => i).filter((i) => rep.terrain[i]), [rep])

  useEffect(() => {
    const el = wrap.current!
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth, h = el.clientHeight
      setDims({ w, h })
      setCam((c) => (c.k === 1 && c.x === 0 && c.y === 0 ? { x: 0, y: 0, k: Math.min(w, h) / (SIZE * Math.sqrt(3) * (2 * rep.radius + 2)) } : c))
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [rep.radius])

  const fogged = (idx: number) => visible !== null && !visible.has(idx)

  useEffect(() => {
    const c = canvas.current!
    const dpr = window.devicePixelRatio || 1
    c.width = dims.w * dpr; c.height = dims.h * dpr
    const ctx = c.getContext('2d')!
    ctx.scale(dpr, dpr)
    ctx.clearRect(0, 0, dims.w, dims.h)
    ctx.translate(dims.w / 2 + cam.x, dims.h / 2 + cam.y)
    ctx.scale(cam.k, cam.k)
    const s = SIZE - 0.8
    const showNums = cam.k * SIZE > 11
    const sel = selected !== null ? hexAt(rep.radius, selected) : null
    const selCell = selected !== null ? cells.get(selected) : undefined

    for (const idx of all) {
      const t = rep.terrain[idx]!
      const h = hexAt(rep.radius, idx)
      const { x, y } = toPx(h)
      hexPath(ctx, x, y, s)
      if (t.kind !== 'open') {
        ctx.fillStyle = t.kind === 'water' ? '#0b2236' : '#15171c'
        ctx.fill()
        ctx.strokeStyle = t.kind === 'water' ? '#17496f' : '#3a3f4b'
        ctx.lineWidth = 1
        ctx.stroke()
        ctx.save(); ctx.clip()
        ctx.strokeStyle = t.kind === 'water' ? '#17496f' : '#3a3f4b'
        ctx.beginPath()
        for (let d = -SIZE; d <= SIZE; d += 6) { ctx.moveTo(x + d - SIZE, y + SIZE); ctx.lineTo(x + d + SIZE, y - SIZE) }
        ctx.stroke(); ctx.restore()
        continue
      }
      const hidden = fogged(idx)
      ctx.fillStyle = HEIGHT_FILL[t.height + 2]
      ctx.fill()
      if (hidden) { ctx.fillStyle = 'rgba(6,7,10,0.62)'; ctx.fill() }
      ctx.strokeStyle = '#0b0d11'; ctx.lineWidth = 0.8; ctx.stroke()
      const cell = cells.get(idx)
      if (hidden) continue
      if (cell && cell.owner >= 0) {
        hexPath(ctx, x, y, s - 1.5)
        ctx.strokeStyle = PLAYER_COLORS[cell.owner % 8]; ctx.lineWidth = 2.6; ctx.stroke()
        ctx.fillStyle = PLAYER_COLORS[cell.owner % 8] + '22'; ctx.fill()
        if (cam.k * SIZE > 7) {
          ctx.fillStyle = PLAYER_COLORS[cell.owner % 8]
          ctx.font = '7px sans-serif'; ctx.textAlign = 'left'; ctx.textBaseline = 'top'
          ctx.fillText(PLAYER_MARKS[cell.owner % 8], x - 4, y - SIZE * 0.78)
        }
      }
      const troops = cell ? cell.troops : rep.neutral[idx]
      if (showNums && troops > 0) {
        ctx.fillStyle = cell && cell.owner >= 0 ? '#fff' : '#cfd3da'
        ctx.font = '600 11px ui-monospace, monospace'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
        ctx.fillText(String(troops), x, y + 2)
      }
      if (cell && cell.building > 0) {
        ctx.fillStyle = '#f4d35e'
        ctx.font = '700 9px sans-serif'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'
        ctx.fillText(BUILDINGS[cell.building - 1], x, y - 9)
      }
    }

    const owned = (b: number) => [...cells].filter(([i, c]) => c.building === b && !fogged(i))
    const farms = owned(1), villages = owned(2), walls = owned(4)
    ctx.lineCap = 'round'
    for (const [fi, f] of farms) {
      const fh = hexAt(rep.radius, fi)
      let best: { d: number; h: Hex } | null = null
      for (const [vi, v] of villages) {
        if (v.owner !== f.owner) continue
        const vh = hexAt(rep.radius, vi), d = hexDist(fh, vh)
        if (d <= 4 && (!best || d < best.d)) best = { d, h: vh }
      }
      if (best) {
        const a = toPx(fh), b = toPx(best.h)
        ctx.setLineDash([4, 3]); ctx.strokeStyle = '#f4d35e'; ctx.lineWidth = 2
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.setLineDash([])
      }
    }
    for (const [wi, w] of walls) {
      const wh = hexAt(rep.radius, wi)
      for (const n of [{ q: wh.q + 1, r: wh.r }, { q: wh.q, r: wh.r + 1 }, { q: wh.q - 1, r: wh.r + 1 }]) {
        if (!onBoard(rep.radius, n)) continue
        const o = cells.get(hexIndex(rep.radius, n))
        if (o?.building === 4 && o.owner === w.owner) {
          const a = toPx(wh), b = toPx(n)
          ctx.strokeStyle = '#e8eaee'; ctx.lineWidth = 4
          ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke()
        }
      }
    }
    if (sel && selCell?.building === 3) {
      const r = 2
      for (const idx of all) {
        const h = hexAt(rep.radius, idx)
        if (hexDist(h, sel) <= r && rep.terrain[idx]?.kind === 'open') {
          const { x, y } = toPx(h)
          hexPath(ctx, x, y, s); ctx.fillStyle = 'rgba(244,211,94,0.16)'; ctx.fill()
        }
      }
    }
    if (sel) {
      const { x, y } = toPx(sel)
      hexPath(ctx, x, y, s + 1); ctx.strokeStyle = '#fff'; ctx.lineWidth = 3; ctx.stroke()
    }
  }, [rep, cells, visible, cam, dims, selected, all, perspective])

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
      ctx.fillStyle = t.kind !== 'open' ? '#262a33' : hidden ? '#14171d' : cell && cell.owner >= 0 ? PLAYER_COLORS[cell.owner % 8] : '#3b4136'
      ctx.fillRect(W / 2 + x * k - 1.5, W / 2 + y * k - 1.5, 3, 3)
    }
    ctx.strokeStyle = '#fff'; ctx.lineWidth = 1
    const vw = dims.w / cam.k * k, vh = dims.h / cam.k * k
    ctx.strokeRect(W / 2 + (-cam.x / cam.k) * k - vw / 2, W / 2 + (-cam.y / cam.k) * k - vh / 2, vw, vh)
  }, [rep, cells, visible, cam, dims, all])

  const pick = (ev: React.MouseEvent) => {
    const r = canvas.current!.getBoundingClientRect()
    const wx = (ev.clientX - r.left - dims.w / 2 - cam.x) / cam.k
    const wy = (ev.clientY - r.top - dims.h / 2 - cam.y) / cam.k
    const rf = wy / (SIZE * 1.5)
    const qf = wx / (SIZE * Math.sqrt(3)) - rf / 2
    let q = Math.round(qf), rr = Math.round(rf), s = Math.round(-qf - rf)
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
        onWheel={(e) => {
          const f = Math.exp(-e.deltaY * 0.0015)
          setCam((c) => ({ ...c, k: Math.min(6, Math.max(0.25, c.k * f)) }))
        }}
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
        <button className="btn" onClick={() => setCam((c) => ({ ...c, k: Math.min(6, c.k * 1.25) }))}>+</button>
        <button className="btn" onClick={() => setCam((c) => ({ ...c, k: Math.max(0.25, c.k / 1.25) }))}>-</button>
        <button className="btn" onClick={() => setCam({ x: 0, y: 0, k: Math.min(dims.w, dims.h) / (SIZE * Math.sqrt(3) * (2 * rep.radius + 2)) })}>Fit</button>
      </div>
    </div>
  )
}
