export const BUILDING_STYLE = [
  { name: 'Farm', color: '#8be36f', desc: 'Valley extractor. 3 support sockets link to adjacent Villages.' },
  { name: 'Village', color: '#ffbd6e', desc: 'Troop producer: 2/tick, or 3/tick when a Farm supports it.' },
  { name: 'Quarry', color: '#6fd6ff', desc: 'Hill extractor. Adjacent builds get 25% off troops and 1 off Resource.' },
  { name: 'Wall', color: '#f1f3f7', desc: 'Defense 2.0x, 2.5x with one link, 3.0x with two linked Walls.' },
] as const

/** Draws a building badge centered on (x, y); `r` is the badge radius in px. */
export function drawBuilding(ctx: CanvasRenderingContext2D, kind: number, x: number, y: number, r: number, opts: { dashed?: boolean; doubleRing?: boolean } = {}) {
  const st = BUILDING_STYLE[kind - 1]
  ctx.save()
  ctx.translate(x, y)
  ctx.beginPath(); ctx.arc(0, 0, r, 0, Math.PI * 2)
  ctx.fillStyle = '#0a0c10'; ctx.fill()
  ctx.lineWidth = Math.max(1.2, r * 0.14); ctx.strokeStyle = st.color
  if (opts.dashed) ctx.setLineDash([r * 0.45, r * 0.3])
  ctx.stroke(); ctx.setLineDash([])
  if (opts.doubleRing) {
    ctx.beginPath(); ctx.arc(0, 0, r * 1.2, 0, Math.PI * 2)
    ctx.lineWidth = Math.max(1, r * 0.1); ctx.stroke()
  }
  const u = r * 0.58
  ctx.strokeStyle = st.color; ctx.fillStyle = st.color
  ctx.lineWidth = Math.max(1, r * 0.13); ctx.lineJoin = 'round'; ctx.lineCap = 'round'
  ctx.beginPath()
  switch (kind) {
    case 1:
      for (const dy of [-0.2, 0.25, 0.7]) { ctx.moveTo(-u, dy * u); ctx.lineTo(u, dy * u) }
      ctx.stroke()
      ctx.beginPath(); ctx.moveTo(0, -0.2 * u); ctx.lineTo(0, -0.95 * u)
      ctx.moveTo(0, -0.55 * u); ctx.lineTo(-0.4 * u, -0.8 * u)
      ctx.moveTo(0, -0.55 * u); ctx.lineTo(0.4 * u, -0.8 * u)
      ctx.stroke()
      break
    case 2:
      ctx.moveTo(-u, -0.05 * u); ctx.lineTo(0, -u); ctx.lineTo(u, -0.05 * u)
      ctx.lineTo(u * 0.7, -0.05 * u); ctx.lineTo(u * 0.7, u * 0.85); ctx.lineTo(-u * 0.7, u * 0.85)
      ctx.lineTo(-u * 0.7, -0.05 * u); ctx.closePath(); ctx.fill()
      ctx.fillStyle = '#0a0c10'; ctx.fillRect(-u * 0.2, u * 0.3, u * 0.4, u * 0.55)
      break
    case 3:
      ctx.moveTo(0, -u); ctx.lineTo(u * 0.75, -u * 0.15); ctx.lineTo(0, u); ctx.lineTo(-u * 0.75, -u * 0.15)
      ctx.closePath(); ctx.fill()
      ctx.strokeStyle = '#0a0c10'; ctx.beginPath()
      ctx.moveTo(-u * 0.75, -u * 0.15); ctx.lineTo(u * 0.75, -u * 0.15); ctx.moveTo(0, -u); ctx.lineTo(-u * 0.25, -u * 0.15)
      ctx.moveTo(0, u); ctx.lineTo(-u * 0.25, -u * 0.15)
      ctx.stroke()
      break
    case 4:
      ctx.strokeRect(-u, -u * 0.8, u * 2, u * 1.6)
      ctx.moveTo(-u, -u * 0.27); ctx.lineTo(u, -u * 0.27)
      ctx.moveTo(-u, u * 0.27); ctx.lineTo(u, u * 0.27)
      ctx.moveTo(0, -u * 0.8); ctx.lineTo(0, -u * 0.27)
      ctx.moveTo(-u * 0.5, -u * 0.27); ctx.lineTo(-u * 0.5, u * 0.27)
      ctx.moveTo(u * 0.5, -u * 0.27); ctx.lineTo(u * 0.5, u * 0.27)
      ctx.moveTo(0, u * 0.27); ctx.lineTo(0, u * 0.8)
      ctx.stroke()
      break
  }
  ctx.restore()
}
