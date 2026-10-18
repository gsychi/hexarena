import { useEffect, useRef } from 'react'
import { BUILDING_STYLE, drawBuilding, drawHeightMark } from './icons'
import type { Pips } from './icons'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { HEIGHT_STYLE } from './model'

export function BuildingIcon({ kind, size = 26, pips }: { kind: number; size?: number; pips?: Pips }) {
  const ref = useRef<HTMLCanvasElement>(null)
  useEffect(() => {
    const c = ref.current!
    const dpr = window.devicePixelRatio || 1
    c.width = size * dpr; c.height = size * dpr
    const ctx = c.getContext('2d')!
    ctx.scale(dpr, dpr)
    const r = size * (pips ? 0.3 : 0.4)
    drawBuilding(ctx, kind, size / 2, pips ? size * 0.58 : size / 2, r, { pips })
  }, [kind, size, pips])
  return <canvas ref={ref} style={{ width: size, height: size }} />
}

function HeightMark({ height }: { height: number }) {
  const ref = useRef<HTMLCanvasElement>(null)
  useEffect(() => {
    const c = ref.current!
    const dpr = window.devicePixelRatio || 1
    c.width = 28 * dpr; c.height = 14 * dpr
    const ctx = c.getContext('2d')!
    ctx.scale(dpr, dpr)
    drawHeightMark(ctx, height, 14, 7, 9)
  }, [height])
  return <canvas ref={ref} style={{ width: 28, height: 14 }} />
}

const FARM = BUILDING_STYLE[0].color
const WALL = BUILDING_STYLE[3].color

export function Legend() {
  return (
    <Card size="sm">
      <CardHeader><CardTitle>Terrain height</CardTitle></CardHeader>
      <CardContent className="space-y-3 text-xs">
        <div className="grid grid-cols-5 gap-1">
          {[2, 1, 0, -1, -2].map((h) => {
            const s = HEIGHT_STYLE[h]
            return (
              <div key={h} className="flex flex-col items-center gap-0.5 rounded-md border py-1.5" style={{ background: s.fill }}>
                <span className="text-[13px] font-bold" style={{ color: s.tag }}>{s.label}</span>
                <span className="flex h-3.5 items-center">{h === 0 ? <span className="text-[9px]" style={{ color: s.tag }}>no mark</span> : <HeightMark height={h} />}</span>
                <span className="text-[9px] leading-tight" style={{ color: s.tag }}>{s.name}</span>
              </div>
            )
          })}
        </div>
        <div className="text-muted-foreground">On the board, ▲ marks a hill and ▼ a valley. Two chevrons mean ±2. Valleys are cheap to take but weak; hills are costly to take but strong.</div>
        <div className="flex items-center gap-2">
          <span className="inline-block h-4 w-4 border border-[#4a4f5c]" style={{ background: 'repeating-linear-gradient(135deg,#101216 0 3px,#4a4f5c 3px 4px)' }} />Rock (impassable)
          <span className="ml-2 inline-block h-4 w-4 border border-[#1b5a8a]" style={{ background: 'repeating-linear-gradient(135deg,#07111c 0 3px,#1b5a8a 3px 4px)' }} />Water (impassable)
        </div>
        <div className="pt-1 text-sm font-medium">Buildings</div>
        {BUILDING_STYLE.map((b, i) => (
          <div key={b.name} className="flex items-center gap-2.5 py-0.5">
            <BuildingIcon kind={i + 1} size={28} />
            <div><span style={{ color: b.color }} className="font-semibold">{b.name}</span> <span className="text-muted-foreground">{b.desc}</span></div>
          </div>
        ))}
        <div className="text-muted-foreground">Dots above a building are its link slots. Filled dots are active links.</div>
        <div className="grid grid-cols-3 gap-1 text-center text-[10px] text-muted-foreground">
          <div className="flex flex-col items-center"><BuildingIcon kind={1} size={34} pips={{ filled: 2, total: 3, color: FARM }} />Farm, 2 of 3 Villages</div>
          <div className="flex flex-col items-center"><BuildingIcon kind={2} size={34} pips={{ filled: 1, total: 1, color: FARM }} />Village with Farm, 3/tick</div>
          <div className="flex flex-col items-center"><BuildingIcon kind={4} size={34} pips={{ filled: 2, total: 2, color: WALL }} />Wall, 2 links, 3.0x</div>
        </div>
      </CardContent>
    </Card>
  )
}
