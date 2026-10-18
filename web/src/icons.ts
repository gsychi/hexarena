export const BUILDING_STYLE = [
  { name: 'Farm', color: '#86e36a', light: '#c8f6b4', ink: '#0f2a0a', desc: 'Valley extractor. Links to up to 3 adjacent Villages.' },
  { name: 'Village', color: '#ffb35c', light: '#ffdcae', ink: '#3a1d02', desc: 'Makes 2 troops/tick, or 3 when a Farm supports it.' },
  { name: 'Quarry', color: '#5cc8f2', light: '#b9eafc', ink: '#062436', desc: 'Hill extractor. Adjacent builds cost 25% fewer troops and 1 less Resource.' },
  { name: 'Wall', color: '#d5d9e1', light: '#ffffff', ink: '#1a1d24', desc: 'Defense 2.0x, 2.5x with one linked Wall, 3.0x with two.' },
] as const

export interface Pips { filled: number; total: number; color: string }

function glyph(ctx: CanvasRenderingContext2D, kind: number, r: number, ink: string, hole: string) {
  const u = r * 0.6
  ctx.fillStyle = ink; ctx.strokeStyle = ink
  ctx.lineCap = 'round'; ctx.lineJoin = 'round'
  switch (kind) {
    case 1: {
      ctx.lineWidth = r * 0.13
      ctx.beginPath(); ctx.moveTo(0, u * 0.95); ctx.lineTo(0, -u * 0.55); ctx.stroke()
      const grain = (gx: number, gy: number, rot: number) => {
        ctx.beginPath(); ctx.ellipse(gx, gy, u * 0.17, u * 0.34, rot, 0, Math.PI * 2); ctx.fill()
      }
      grain(0, -u * 0.72, 0)
      for (const gy of [-u * 0.3, u * 0.15, u * 0.58]) {
        grain(-u * 0.3, gy - u * 0.05, -0.65)
        grain(u * 0.3, gy - u * 0.05, 0.65)
      }
      break
    }
    case 2:
      ctx.beginPath()
      ctx.moveTo(-u * 1.02, -u * 0.02); ctx.lineTo(0, -u * 0.95); ctx.lineTo(u * 1.02, -u * 0.02); ctx.closePath()
      ctx.fill()
      ctx.fillRect(u * 0.42, -u * 0.82, u * 0.24, u * 0.5)
      ctx.fillRect(-u * 0.7, -u * 0.1, u * 1.4, u * 0.98)
      ctx.fillStyle = hole
      ctx.beginPath(); ctx.roundRect(-u * 0.2, u * 0.3, u * 0.4, u * 0.58, [u * 0.2, u * 0.2, 0, 0]); ctx.fill()
      break
    case 3:
      ctx.save(); ctx.rotate(-0.55)
      ctx.lineWidth = r * 0.2
      ctx.beginPath(); ctx.moveTo(-u * 0.98, -u * 0.18); ctx.quadraticCurveTo(0, -u * 1.05, u * 0.98, -u * 0.18); ctx.stroke()
      ctx.lineWidth = r * 0.17
      ctx.beginPath(); ctx.moveTo(0, -u * 0.6); ctx.lineTo(0, u * 1.0); ctx.stroke()
      ctx.restore()
      break
    case 4: {
      const w = u * 0.86
      ctx.beginPath()
      ctx.moveTo(-w, u * 0.88); ctx.lineTo(-w, -u * 0.82)
      for (let i = 0; i < 3; i++) {
        const x0 = -w + i * w * 0.8
        ctx.lineTo(x0 + w * 0.4, -u * 0.82); ctx.lineTo(x0 + w * 0.4, -u * 0.42); ctx.lineTo(x0 + w * 0.8, -u * 0.42); ctx.lineTo(x0 + w * 0.8, -u * 0.82)
      }
      ctx.lineTo(w, -u * 0.82); ctx.lineTo(w, u * 0.88); ctx.closePath(); ctx.fill()
      ctx.fillStyle = hole
      ctx.beginPath(); ctx.moveTo(-u * 0.27, u * 0.88); ctx.lineTo(-u * 0.27, u * 0.32); ctx.arc(0, u * 0.32, u * 0.27, Math.PI, 0); ctx.lineTo(u * 0.27, u * 0.88); ctx.closePath(); ctx.fill()
      break
    }
  }
}

/**
 * Draws a building token centered on (x, y) with radius `r`. Optional pips sit
 * on an arc above the token and show filled link slots.
 */
export function drawBuilding(ctx: CanvasRenderingContext2D, kind: number, x: number, y: number, r: number, opts: { pips?: Pips; glyph?: boolean } = {}) {
  const st = BUILDING_STYLE[kind - 1]
  ctx.save()
  ctx.translate(x, y)
  ctx.beginPath(); ctx.arc(0, 0, r + Math.max(1.2, r * 0.16), 0, Math.PI * 2)
  ctx.fillStyle = 'rgba(6,7,10,0.88)'; ctx.fill()
  const g = ctx.createLinearGradient(0, -r, 0, r)
  g.addColorStop(0, st.light); g.addColorStop(0.55, st.color)
  ctx.beginPath(); ctx.arc(0, 0, r, 0, Math.PI * 2)
  ctx.fillStyle = g; ctx.fill()
  ctx.lineWidth = Math.max(0.6, r * 0.07); ctx.strokeStyle = 'rgba(255,255,255,0.45)'
  ctx.beginPath(); ctx.arc(0, 0, r - ctx.lineWidth, Math.PI * 1.1, Math.PI * 1.9); ctx.stroke()
  if (opts.glyph !== false) glyph(ctx, kind, r, st.ink, st.color)
  const p = opts.pips
  if (p && p.total > 0) {
    const pr = Math.max(1.1, r * 0.17), ring = r + pr * 2.1, gap = (pr * 2.6) / ring
    for (let i = 0; i < p.total; i++) {
      const a = -Math.PI / 2 + (i - (p.total - 1) / 2) * gap
      const px = Math.cos(a) * ring, py = Math.sin(a) * ring
      ctx.beginPath(); ctx.arc(px, py, pr + 0.9, 0, Math.PI * 2); ctx.fillStyle = 'rgba(6,7,10,0.92)'; ctx.fill()
      ctx.beginPath(); ctx.arc(px, py, pr, 0, Math.PI * 2)
      if (i < p.filled) { ctx.fillStyle = p.color; ctx.fill() } else { ctx.lineWidth = Math.max(0.7, pr * 0.45); ctx.strokeStyle = p.color + '99'; ctx.stroke() }
    }
  }
  ctx.restore()
}

/** Up chevrons for hills, down chevrons for valleys; plains draw nothing. */
export function drawHeightMark(ctx: CanvasRenderingContext2D, height: number, x: number, y: number, size: number) {
  const n = Math.abs(height)
  if (n === 0) return
  const up = height > 0
  const w = size, h = size * 0.78, gap = size * 0.22
  const total = n * w + (n - 1) * gap
  ctx.save()
  ctx.lineJoin = 'round'
  for (let i = 0; i < n; i++) {
    const cx = x - total / 2 + w / 2 + i * (w + gap)
    ctx.beginPath()
    if (up) { ctx.moveTo(cx - w / 2, y + h / 2); ctx.lineTo(cx, y - h / 2); ctx.lineTo(cx + w / 2, y + h / 2) }
    else { ctx.moveTo(cx - w / 2, y - h / 2); ctx.lineTo(cx, y + h / 2); ctx.lineTo(cx + w / 2, y - h / 2) }
    ctx.closePath()
    ctx.lineWidth = 2.2; ctx.strokeStyle = 'rgba(6,7,10,0.85)'; ctx.stroke()
    ctx.fillStyle = up ? '#f4f5f7' : '#c4b5fd'; ctx.fill()
  }
  ctx.restore()
}
