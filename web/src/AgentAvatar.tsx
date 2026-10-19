/**
 * The agents designing bots, drawn as little researchers in the same flat style as the
 * bot mascots: a soft creature in a lab coat with glasses or goggles, a hairdo or hat,
 * and a research prop. Every look is stable for an agent's id. Claude agents wear warm
 * colours and GPT agents cool ones.
 */
import { hash, INK, PALETTES } from './BotAvatar'
import type { Palette } from './BotAvatar'

type Glasses = 'round' | 'square' | 'goggles' | 'monocle'
type Top = 'none' | 'tuft' | 'bun' | 'beanie' | 'cap' | 'curls'
type Prop = 'pencil' | 'clipboard' | 'magnifier' | 'flask'
type Ears = 'none' | 'round' | 'pointy'

const GLASSES: Glasses[] = ['round', 'square', 'goggles', 'monocle']
const TOPS: Top[] = ['none', 'tuft', 'bun', 'beanie', 'cap', 'curls']
const PROPS: Prop[] = ['pencil', 'clipboard', 'magnifier', 'flask']
const EARS: Ears[] = ['none', 'round', 'pointy']

/** Palette indices into the bot mascot palettes. */
const WARM = [0, 1, 6, 8, 5]
const COOL = [7, 2, 4, 9, 3]

const COAT = '#F3F0EA'
const COAT_SHADE = '#D6D0C4'

interface Look { palette: Palette; shirt: Palette; glasses: Glasses; top: Top; prop: Prop; ears: Ears }

function lookOf(id: string, model: string | null): Look {
  const h = hash(`researcher:${id}`)
  const pool = model?.toLowerCase() === 'claude' ? WARM : model?.toLowerCase() === 'gpt' ? COOL : [...WARM, ...COOL]
  const palette = PALETTES[pool[h % pool.length]]
  const shirt = PALETTES[(h >>> 4) % PALETTES.length]
  return {
    palette,
    shirt: shirt === palette ? PALETTES[(PALETTES.indexOf(palette) + 3) % PALETTES.length] : shirt,
    glasses: GLASSES[(h >>> 8) % GLASSES.length],
    top: TOPS[(h >>> 12) % TOPS.length],
    prop: PROPS[(h >>> 16) % PROPS.length],
    ears: EARS[(h >>> 20) % EARS.length],
  }
}

function EarShapes({ kind, p }: { kind: Ears; p: Palette }) {
  switch (kind) {
    case 'none': return null
    case 'round':
      return (
        <g>
          <circle cx={27} cy={31} r={8.5} fill={p.body} />
          <circle cx={73} cy={31} r={8.5} fill={p.body} />
          <circle cx={27} cy={31} r={4.5} fill={p.light} />
          <circle cx={73} cy={31} r={4.5} fill={p.light} />
        </g>
      )
    case 'pointy':
      return (
        <g strokeLinejoin="round">
          <path d="M24 40L25 18L40 30Z" fill={p.body} stroke={p.body} strokeWidth={3} />
          <path d="M76 40L75 18L60 30Z" fill={p.body} stroke={p.body} strokeWidth={3} />
          <path d="M28 34L28.5 24L35 30Z" fill={p.light} />
          <path d="M72 34L71.5 24L65 30Z" fill={p.light} />
        </g>
      )
    default: { const never: never = kind; return never }
  }
}

function TopShape({ kind, p, shirt }: { kind: Top; p: Palette; shirt: Palette }) {
  switch (kind) {
    case 'none': return null
    case 'tuft':
      return <path d="M44 27C44 19 49 16 51 21C52 15 58 16 56 24" fill="none" stroke={p.shade} strokeWidth={4} strokeLinecap="round" />
    case 'bun':
      return (
        <g>
          <circle cx={50} cy={22} r={8} fill={p.shade} />
          <path d="M43 25Q50 29 57 25" fill="none" stroke={shirt.body} strokeWidth={2.4} strokeLinecap="round" />
        </g>
      )
    case 'beanie':
      return (
        <g>
          <path d="M26 39C26 20 74 20 74 39Z" fill={shirt.body} />
          <rect x={24} y={35} width={52} height={8} rx={4} fill={shirt.shade} />
          <circle cx={50} cy={20} r={4.5} fill={shirt.light} />
        </g>
      )
    case 'cap':
      return (
        <g strokeLinejoin="round">
          <path d="M30 33L50 24L70 33L50 41Z" fill={INK} stroke={INK} strokeWidth={1.5} />
          <path d="M37 35L37 41Q50 47 63 41L63 35L50 41Z" fill={INK} />
          <path d="M66 32L68 44" stroke="#F2C14E" strokeWidth={1.6} strokeLinecap="round" />
          <circle cx={68} cy={45} r={2} fill="#F2C14E" />
        </g>
      )
    case 'curls':
      return (
        <g fill={p.shade}>
          <circle cx={38} cy={28} r={6} />
          <circle cx={47} cy={24} r={6.5} />
          <circle cx={57} cy={25} r={6} />
          <circle cx={65} cy={30} r={5} />
        </g>
      )
    default: { const never: never = kind; return never }
  }
}

function Eyes() {
  return (
    <g>
      <circle cx={39} cy={52} r={3.4} fill={INK} />
      <circle cx={61} cy={52} r={3.4} fill={INK} />
      <circle cx={40.2} cy={50.8} r={1.1} fill="#fff" />
      <circle cx={62.2} cy={50.8} r={1.1} fill="#fff" />
    </g>
  )
}

function GlassesShape({ kind }: { kind: Glasses }) {
  const frame = { fill: '#fff', fillOpacity: 0.16, stroke: INK, strokeWidth: 2.4 }
  switch (kind) {
    case 'round':
      return (
        <g>
          <circle cx={39} cy={52} r={8.5} {...frame} />
          <circle cx={61} cy={52} r={8.5} {...frame} />
          <path d="M47.5 51Q50 49 52.5 51" fill="none" stroke={INK} strokeWidth={2.2} />
        </g>
      )
    case 'square':
      return (
        <g>
          <rect x={30} y={45} width={18} height={14} rx={3.5} {...frame} />
          <rect x={52} y={45} width={18} height={14} rx={3.5} {...frame} />
          <path d="M48 50L52 50" stroke={INK} strokeWidth={2.2} />
        </g>
      )
    case 'goggles':
      return (
        <g>
          <path d="M24 51L76 51" stroke="#5B6B7A" strokeWidth={4} />
          <rect x={29} y={44} width={20} height={16} rx={7} fill="#9FD6F2" fillOpacity={0.35} stroke="#5B6B7A" strokeWidth={3} />
          <rect x={51} y={44} width={20} height={16} rx={7} fill="#9FD6F2" fillOpacity={0.35} stroke="#5B6B7A" strokeWidth={3} />
          <path d="M33 48L37 46" stroke="#fff" strokeWidth={1.6} strokeLinecap="round" opacity={0.8} />
          <path d="M55 48L59 46" stroke="#fff" strokeWidth={1.6} strokeLinecap="round" opacity={0.8} />
        </g>
      )
    case 'monocle':
      return (
        <g>
          <circle cx={61} cy={52} r={9} {...frame} />
          <path d="M69 57Q74 66 70 74" fill="none" stroke="#C9A24A" strokeWidth={1.4} />
        </g>
      )
    default: { const never: never = kind; return never }
  }
}

function Coat({ shirt }: { shirt: Palette }) {
  return (
    <g>
      <path d="M12 100C12 86 22 77 36 75L64 75C78 77 88 86 88 100Z" fill={COAT} />
      <path d="M41 75L50 92L59 75Z" fill={shirt.body} />
      <path d="M41 75L50 92L59 75" fill="none" stroke={COAT_SHADE} strokeWidth={2} strokeLinejoin="round" />
      <path d="M41 75L45 84L36 82Z" fill={COAT_SHADE} opacity={0.7} />
      <path d="M59 75L55 84L64 82Z" fill={COAT_SHADE} opacity={0.7} />
      <path d="M50 92L50 100" stroke={COAT_SHADE} strokeWidth={1.6} />
      <rect x={24} y={87} width={11} height={8} rx={1.5} fill="none" stroke={COAT_SHADE} strokeWidth={1.6} />
      <rect x={27} y={83} width={2.4} height={7} rx={1} fill="#4A7BD9" />
      <rect x={30.5} y={84} width={2.4} height={6} rx={1} fill="#E0565B" />
    </g>
  )
}

function PropShape({ kind }: { kind: Prop }) {
  switch (kind) {
    case 'pencil':
      return (
        <g transform="rotate(-38 76 34)">
          <rect x={70} y={31} width={18} height={5} rx={1} fill="#F2C14E" />
          <rect x={86} y={31} width={3} height={5} fill="#F29BB4" />
          <path d="M70 31L65 33.5L70 36Z" fill="#E5C9A8" />
          <path d="M66.5 32.8L65 33.5L66.5 34.2Z" fill={INK} />
        </g>
      )
    case 'clipboard':
      return (
        <g transform="rotate(10 74 86)">
          <rect x={64} y={76} width={20} height={25} rx={2.5} fill="#B9875A" />
          <rect x={66.5} y={80} width={15} height={19} rx={1} fill="#fff" />
          <rect x={70} y={74.5} width={8} height={4} rx={1.5} fill="#8A8F98" />
          <path d="M69 85L79 85M69 89L79 89M69 93L75 93" stroke="#9AA3AF" strokeWidth={1.3} strokeLinecap="round" />
          <path d="M68.5 84.5l1.4 1.4 2.4-2.6" fill="none" stroke="#3FB27F" strokeWidth={1.2} />
        </g>
      )
    case 'magnifier':
      return (
        <g>
          <path d="M80 94L88 102" stroke="#7A5638" strokeWidth={4.5} strokeLinecap="round" />
          <circle cx={74} cy={87} r={8.5} fill="#BFE2F9" fillOpacity={0.45} stroke="#5B6B7A" strokeWidth={2.8} />
          <path d="M70 84Q72 81 75 81" fill="none" stroke="#fff" strokeWidth={1.6} strokeLinecap="round" />
        </g>
      )
    case 'flask':
      return (
        <g>
          <path d="M71 78L71 85L64 97Q63 100 66 100L82 100Q85 100 84 97L77 85L77 78Z" fill="#fff" fillOpacity={0.85} stroke="#8A8F98" strokeWidth={1.6} strokeLinejoin="round" />
          <path d="M66.5 93L81.5 93L84 97Q85 100 82 100L66 100Q63 100 64 97Z" fill="#7FD8C3" />
          <rect x={69.5} y={76} width={9} height={3} rx={1.5} fill="#8A8F98" />
          <circle cx={72} cy={90} r={1.3} fill="#7FD8C3" />
          <circle cx={75} cy={86.5} r={1} fill="#7FD8C3" />
        </g>
      )
    default: { const never: never = kind; return never }
  }
}

export function AgentAvatar({ id, model, size = 44, className }: { id: string; model: string | null; size?: number; className?: string }) {
  const l = lookOf(id, model)
  const p = l.palette
  const clip = `agent-clip-${id.replace(/[^a-zA-Z0-9_-]/g, '-')}`
  return (
    <svg viewBox="0 0 100 100" width={size} height={size} className={className} role="img" aria-label="Researcher">
      <defs>
        <clipPath id={clip}><circle cx={50} cy={50} r={50} /></clipPath>
      </defs>
      <g clipPath={`url(#${clip})`}>
        <circle cx={50} cy={50} r={50} fill={p.bg} />
        <EarShapes kind={l.ears} p={p} />
        <Coat shirt={l.shirt} />
        <ellipse cx={50} cy={50} rx={27} ry={25} fill={p.body} />
        <ellipse cx={37} cy={37} rx={6} ry={3.4} transform="rotate(-32 37 37)" fill="#fff" opacity={0.3} />
        <TopShape kind={l.top} p={p} shirt={l.shirt} />
        <Eyes />
        <GlassesShape kind={l.glasses} />
        <g fill={p.blush} opacity={0.55}>
          <ellipse cx={30} cy={63} rx={4.5} ry={2.6} />
          <ellipse cx={70} cy={63} rx={4.5} ry={2.6} />
        </g>
        <path d="M45 64Q50 68.5 55 64" fill="none" stroke={INK} strokeWidth={2.4} strokeLinecap="round" />
        <PropShape kind={l.prop} />
      </g>
    </svg>
  )
}
