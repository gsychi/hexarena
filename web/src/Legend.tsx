import { useEffect, useRef } from 'react'
import { BUILDING_STYLE, drawBuilding } from './icons'
import { HEIGHT_STYLE } from './model'

export function BuildingIcon({ kind, size = 26, dashed, double }: { kind: number; size?: number; dashed?: boolean; double?: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null)
  useEffect(() => {
    const c = ref.current!
    const dpr = window.devicePixelRatio || 1
    c.width = size * dpr; c.height = size * dpr
    const ctx = c.getContext('2d')!
    ctx.scale(dpr, dpr)
    drawBuilding(ctx, kind, size / 2, size / 2, size * 0.38, { dashed, doubleRing: double })
  }, [kind, size, dashed, double])
  return <canvas ref={ref} style={{ width: size, height: size }} />
}

export function Legend() {
  return (
    <div className="panel p-3">
      <div className="mb-2 text-[10px] tracking-widest text-neutral-500">TERRAIN HEIGHT</div>
      <div className="grid grid-cols-5 gap-1">
        {[-2, -1, 0, 1, 2].map((h) => {
          const s = HEIGHT_STYLE[h]
          return (
            <div key={h} className="flex flex-col items-center rounded-md border border-[#1c2029] py-1" style={{ background: s.fill }}>
              <span className="text-[13px] font-bold" style={{ color: s.tag }}>{s.label}</span>
              <span className="text-[9px] leading-tight" style={{ color: s.tag }}>{s.name}</span>
            </div>
          )
        })}
      </div>
      <div className="mt-1 text-[10px] text-neutral-500">Valleys are cheap to take but weak. Hills are costly to take but strong. Every owned hex still makes 1 troop/tick.</div>
      <div className="mt-2 flex items-center gap-2 text-[11px]">
        <span className="inline-block h-4 w-4 border border-[#4a4f5c]" style={{ background: 'repeating-linear-gradient(135deg,#101216 0 3px,#4a4f5c 3px 4px)' }} />Rock (impassable)
        <span className="ml-2 inline-block h-4 w-4 border border-[#1b5a8a]" style={{ background: 'repeating-linear-gradient(135deg,#07111c 0 3px,#1b5a8a 3px 4px)' }} />Water (impassable)
      </div>
      <div className="mb-1 mt-3 text-[10px] tracking-widest text-neutral-500">BUILDINGS</div>
      {BUILDING_STYLE.map((b, i) => (
        <div key={b.name} className="flex items-center gap-2 py-0.5">
          <BuildingIcon kind={i + 1} />
          <div><span style={{ color: b.color }} className="font-semibold">{b.name}</span> <span className="text-neutral-400">{b.desc}</span></div>
        </div>
      ))}
      <div className="mt-2 grid grid-cols-3 gap-1 text-center text-[10px] text-neutral-400">
        <div><BuildingIcon kind={2} dashed />Village 2/tick<br />dashed ring</div>
        <div><BuildingIcon kind={2} double />Village 3/tick<br />double ring + Farm mark</div>
        <div><BuildingIcon kind={4} double />Wall 2/2 links<br />double ring</div>
      </div>
    </div>
  )
}
