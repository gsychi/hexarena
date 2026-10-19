/**
 * Cute flat mascots for bots, drawn in SVG. Each bot gets a stable look from its id:
 * a soft body, simple face and colour, plus ears or a crest that follow the animal in
 * its name (rabbit ears for Burrow and Snowshoe, horns for Ridgeline, and so on).
 * A bot with parents inherits instead: ears from its first parent (the one it takes the
 * most policy from), each other feature from one of its parents, one feature of its
 * own, and its parent's colour shifted a little, so families look alike but no two
 * siblings match.
 */
import { lineageOf, useLineageVersion } from './lineage'


type Family = 'rabbit' | 'round' | 'cat' | 'horned' | 'spiky' | 'tuft' | 'fin' | 'tentacle' | 'mask' | 'antenna' | 'sprout'
type Body = 'blob' | 'round' | 'bean' | 'gumdrop' | 'blossom'
type Eyes = 'dots' | 'oval' | 'happy' | 'sleepy' | 'big'
type Mouth = 'smile' | 'cat' | 'o' | 'tongue'

interface Palette { body: string; shade: string; light: string; blush: string; bg: string }

const PALETTES: Palette[] = [
  { body: '#D97757', shade: '#B85C3E', light: '#F2B59C', blush: '#F28B82', bg: '#3A221A' }, // terracotta
  { body: '#F4A582', shade: '#D9825F', light: '#FBD3BF', blush: '#F27A7A', bg: '#3B2620' }, // peach
  { body: '#8FBF9F', shade: '#6C9C7D', light: '#C5E3CE', blush: '#F29B9B', bg: '#1E2E24' }, // sage
  { body: '#B4A7F5', shade: '#8F80DB', light: '#DCD4FB', blush: '#F29BC4', bg: '#25213D' }, // lavender
  { body: '#7CC4F2', shade: '#57A2D3', light: '#BFE2F9', blush: '#F2A0B0', bg: '#172A38' }, // sky
  { body: '#F2D16B', shade: '#D4AF45', light: '#F9E8B0', blush: '#F2957A', bg: '#38301A' }, // butter
  { body: '#F29BB4', shade: '#D57693', light: '#F9CBD8', blush: '#E8607F', bg: '#3A2029' }, // rose
  { body: '#7FD8C3', shade: '#58B7A1', light: '#C0EEE3', blush: '#F2A0A0', bg: '#16302B' }, // mint
  { body: '#E5C9A8', shade: '#C7A783', light: '#F4E6D5', blush: '#F2998A', bg: '#332A20' }, // sand
  { body: '#A3B8F0', shade: '#7F95D1', light: '#D3DDF8', blush: '#F2A0C0', bg: '#1E2539' }, // periwinkle
]
const INK = '#2B1D18'

const FAMILIES: [RegExp, Family][] = ([
  ['bunny|rabbit|hare|snowshoe|burrow|warren|thumper|jackrabbit', 'rabbit'],
  ['porcupine|hedgehog|pangolin|echidna', 'spiky'],
  ['raccoon|raider', 'mask'],
  ['goat|ibex|ram\\b|muskox|bison|yak|cow|bull|moose|elk|deer|pronghorn|caribou|antelope|reindeer|ridgeline', 'horned'],
  ['cat\\b|kitten|lynx|bobcat|ocelot|cheetah|leopard|jaguar|panther|tiger|lion|fox|wolf|hyena|jackal|coyote|dog|puppy', 'cat'],
  ['bear|panda|koala|otter|beaver|marmot|squirrel|chipmunk|hamster|capybara|mouse|pika|vole|mole\\b|badger|mongoose|wolverine|stoat|marten|mink|ermine|weasel|ferret|monkey|lemur|homesteader', 'round'],
  ['owl|eagle|hawk|falcon|bird|parrot|penguin|duck|swan|relay|warden', 'tuft'],
  ['shark|dolphin|whale|orca|fish|predator', 'fin'],
  ['octopus|squid|jelly|kraken|expander', 'tentacle'],
] as [string, Family][]).map(([alts, f]): [RegExp, Family] => [new RegExp(`\\b(?:${alts})`), f])

function hash(s: string): number {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619) >>> 0
  return h
}

function family(id: string, name: string, h: number): Family {
  const bare = name.replace(/^(gpt|claude)-/i, '')
  const [title, subtitle = ''] = bare.split(' — ')
  for (const text of [id, title, subtitle]) {
    const t = text.toLowerCase()
    for (const [re, f] of FAMILIES) if (re.test(t)) return f
  }
  return h % 2 ? 'antenna' : 'sprout'
}

const BODIES: Body[] = ['blob', 'round', 'bean', 'gumdrop', 'blossom']
const EYES: Eyes[] = ['dots', 'oval', 'happy', 'dots', 'sleepy', 'oval']
const MOUTHS: Mouth[] = ['smile', 'cat', 'o', 'smile', 'tongue']

interface Traits { family: Family; palette: Palette; body: Body; eyes: Eyes; mouth: Mouth; blush: boolean; teeth: boolean }

function ownTraits(id: string, name: string): Traits {
  const h = hash(id)
  const pick = <T,>(xs: readonly T[], shift: number) => xs[(h >>> shift) % xs.length]
  const fam = family(id, name, h)
  const owlish = /owl|warden/i.test(id + name)
  return {
    family: fam,
    palette: pick(PALETTES, 0),
    body: pick(BODIES, 5),
    eyes: owlish ? 'big' : pick(EYES, 9),
    mouth: pick(MOUTHS, 13),
    blush: ((h >>> 17) & 3) !== 0,
    teeth: (fam === 'rabbit' || /beaver|capybara/i.test(name)) && ((h >>> 19) & 1) === 1,
  }
}

function hexToHsl(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16)
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255)
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2
  if (max === min) return [0, 0, l]
  const d = max - min
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4
  return [h * 60, s, l]
}

function hslToHex(h: number, s: number, l: number) {
  const f = (n: number) => {
    const k = (n + h / 30) % 12
    const c = l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k - 3, 9 - k, 1))
    return Math.round(c * 255).toString(16).padStart(2, '0')
  }
  return `#${f(0)}${f(8)}${f(4)}`
}

/** The same palette with every colour's hue turned by `deg`. */
function shiftPalette(p: Palette, deg: number): Palette {
  const turn = (hex: string) => { const [h, s, l] = hexToHsl(hex); return hslToHex((h + deg + 360) % 360, s, l) }
  return { body: turn(p.body), shade: turn(p.shade), light: turn(p.light), blush: p.blush, bg: turn(p.bg) }
}

function traitsOf(id: string, name: string, seen: Set<string>): Traits {
  const own = ownTraits(id, name)
  const parents = (lineageOf(id)?.parents ?? []).filter((p) => !seen.has(p))
  if (parents.length === 0) return own
  const lineage = new Set(seen).add(id)
  const from = parents.map((p) => traitsOf(p, lineageOf(p)?.name ?? p, lineage))
  const h = hash(`${id}#kid`)
  const parent = (shift: number) => from[(h >>> shift) % from.length]
  const kid: Traits = {
    family: from[0].family,
    palette: shiftPalette(parent(2).palette, (h & 1 ? 1 : -1) * (14 + ((h >>> 4) % 16))),
    body: parent(9).body,
    eyes: parent(11).eyes,
    mouth: parent(13).mouth,
    blush: parent(15).blush,
    teeth: parent(17).teeth,
  }
  switch ((h >>> 20) % 3) {
    case 0: kid.body = own.body === kid.body ? BODIES[(BODIES.indexOf(own.body) + 1) % BODIES.length] : own.body; break
    case 1: kid.eyes = own.eyes === kid.eyes ? EYES[(EYES.indexOf(own.eyes) + 1) % EYES.length] : own.eyes; break
    default: kid.mouth = own.mouth === kid.mouth ? MOUTHS[(MOUTHS.indexOf(own.mouth) + 1) % MOUTHS.length] : own.mouth
  }
  return kid
}

export function avatarTraits(id: string, name: string): Traits {
  return traitsOf(id, name, new Set())
}

function BodyShape({ kind, fill }: { kind: Body; fill: string }) {
  switch (kind) {
    case 'round': return <circle cx={50} cy={58} r={30} fill={fill} />
    case 'blob': return <path d="M50 28C72 28 80 38 80 58C80 78 70 88 50 88C30 88 20 78 20 58C20 38 28 28 50 28Z" fill={fill} />
    case 'bean': return <ellipse cx={50} cy={58} rx={27} ry={31} fill={fill} />
    case 'gumdrop': return <path d="M50 27C64 27 82 62 81 76C80 88 66 89 50 89C34 89 20 88 19 76C18 62 36 27 50 27Z" fill={fill} />
    case 'blossom': return (
      <g fill={fill}>
        {[0, 60, 120, 180, 240, 300].map((a) => {
          const r = (a * Math.PI) / 180
          return <circle key={a} cx={50 + Math.cos(r) * 15} cy={59 + Math.sin(r) * 15} r={15} />
        })}
        <circle cx={50} cy={59} r={20} />
      </g>
    )
    default: { const never: never = kind; return never }
  }
}

function Behind({ fam, p }: { fam: Family; p: Palette }) {
  switch (fam) {
    case 'rabbit': return (
      <g>
        <ellipse cx={38} cy={22} rx={7.5} ry={18} transform="rotate(-12 38 22)" fill={p.body} />
        <ellipse cx={62} cy={22} rx={7.5} ry={18} transform="rotate(12 62 22)" fill={p.body} />
        <ellipse cx={38} cy={23} rx={3.6} ry={12} transform="rotate(-12 38 23)" fill={p.blush} opacity={0.7} />
        <ellipse cx={62} cy={23} rx={3.6} ry={12} transform="rotate(12 62 23)" fill={p.blush} opacity={0.7} />
      </g>
    )
    case 'round':
    case 'mask': return (
      <g>
        <circle cx={29} cy={34} r={10} fill={fam === 'mask' ? p.shade : p.body} />
        <circle cx={71} cy={34} r={10} fill={fam === 'mask' ? p.shade : p.body} />
        <circle cx={29} cy={34} r={5} fill={p.light} />
        <circle cx={71} cy={34} r={5} fill={p.light} />
      </g>
    )
    case 'cat': return (
      <g fill={p.body} stroke={p.body} strokeWidth={4} strokeLinejoin="round">
        <path d="M24 46L29 17L47 32Z" />
        <path d="M76 46L71 17L53 32Z" />
        <path d="M29 38L31 24L40 32Z" fill={p.blush} stroke="none" opacity={0.7} />
        <path d="M71 38L69 24L60 32Z" fill={p.blush} stroke="none" opacity={0.7} />
      </g>
    )
    case 'horned': return (
      <g fill="none" stroke="#F1E3C6" strokeWidth={6} strokeLinecap="round">
        <path d="M37 34C29 26 30 15 39 12" />
        <path d="M63 34C71 26 70 15 61 12" />
      </g>
    )
    case 'spiky': return (
      <g fill={p.shade} stroke={p.shade} strokeWidth={3} strokeLinejoin="round">
        {[-62, -36, -12, 12, 36, 62].map((a) => (
          <path key={a} d="M43 36L50 14L57 36Z" transform={`rotate(${a} 50 58)`} />
        ))}
      </g>
    )
    case 'tuft': return (
      <g fill={p.shade} stroke={p.shade} strokeWidth={3} strokeLinejoin="round">
        <path d="M30 40L27 21L43 32Z" />
        <path d="M70 40L73 21L57 32Z" />
      </g>
    )
    case 'fin': return <path d="M44 32L60 9C64 18 65 27 62 34Z" fill={p.shade} stroke={p.shade} strokeWidth={3} strokeLinejoin="round" />
    case 'tentacle': return (
      <g fill={p.body}>
        {[27, 39, 50, 61, 73].map((x) => <circle key={x} cx={x} cy={86} r={7} />)}
      </g>
    )
    case 'antenna': return (
      <g>
        <path d="M50 30C50 22 52 18 56 14" fill="none" stroke={INK} strokeWidth={3} strokeLinecap="round" />
        <circle cx={57} cy={12} r={5.5} fill={p.blush} />
        <circle cx={55.5} cy={10.5} r={1.6} fill="#fff" opacity={0.8} />
      </g>
    )
    case 'sprout': return (
      <g>
        <path d="M50 30V18" stroke="#5FAE6E" strokeWidth={3} strokeLinecap="round" />
        <path d="M50 20C44 12 36 13 34 17C38 22 45 23 50 20Z" fill="#7CCB86" />
        <path d="M50 18C55 9 64 9 67 13C63 19 56 21 50 18Z" fill="#8FD898" />
      </g>
    )
    default: { const never: never = fam; return never }
  }
}

function Eye({ x, kind }: { x: number; kind: Eyes }) {
  const y = 56
  switch (kind) {
    case 'dots': return <g><circle cx={x} cy={y} r={4.2} fill={INK} /><circle cx={x - 1.4} cy={y - 1.5} r={1.4} fill="#fff" /></g>
    case 'oval': return <g><ellipse cx={x} cy={y} rx={3.6} ry={5.6} fill={INK} /><circle cx={x - 1.2} cy={y - 2.2} r={1.5} fill="#fff" /></g>
    case 'happy': return <path d={`M${x - 5} ${y + 1.5}Q${x} ${y - 5} ${x + 5} ${y + 1.5}`} fill="none" stroke={INK} strokeWidth={3.2} strokeLinecap="round" />
    case 'sleepy': return <path d={`M${x - 5} ${y - 1}Q${x} ${y + 4.5} ${x + 5} ${y - 1}`} fill="none" stroke={INK} strokeWidth={3.2} strokeLinecap="round" />
    case 'big': return <g><circle cx={x} cy={y} r={8.5} fill="#FFF8EE" /><circle cx={x} cy={y + 0.5} r={4.5} fill={INK} /><circle cx={x - 1.5} cy={y - 1.2} r={1.5} fill="#fff" /></g>
    default: { const never: never = kind; return never }
  }
}

function MouthShape({ kind, teeth }: { kind: Mouth; teeth: boolean }) {
  const stroke = { fill: 'none', stroke: INK, strokeWidth: 2.6, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const }
  const tooth = teeth && <rect x={47.2} y={67.5} width={5.6} height={5} rx={1.2} fill="#fff" stroke={INK} strokeWidth={1.2} />
  switch (kind) {
    case 'smile': return <g>{tooth}<path d="M45 66Q50 71 55 66" {...stroke} /></g>
    case 'cat': return <g>{tooth}<path d="M43.5 66Q46.8 70 50 66.5Q53.2 70 56.5 66" {...stroke} /></g>
    case 'o': return <ellipse cx={50} cy={67.5} rx={2.6} ry={3.1} fill={INK} />
    case 'tongue': return (
      <g>
        <path d="M44.5 65.5Q50 72.5 55.5 65.5Z" fill={INK} stroke={INK} strokeWidth={1.5} strokeLinejoin="round" />
        <path d="M47.3 68.6Q50 71.6 52.7 68.6Q50 67.4 47.3 68.6Z" fill="#F28B9B" />
      </g>
    )
    default: { const never: never = kind; return never }
  }
}

export function BotAvatar({ id, name, size = 40, ring = true, className }: { id: string; name: string; size?: number; ring?: boolean; className?: string }) {
  useLineageVersion()
  const t = avatarTraits(id, name)
  const p = t.palette
  const eyeGap = t.eyes === 'big' ? 11 : 10
  return (
    <svg viewBox="0 0 100 100" width={size} height={size} className={className} role="img" aria-label={`${name} mascot`}>
      {ring && <circle cx={50} cy={50} r={50} fill={p.bg} />}
      <Behind fam={t.family} p={p} />
      <BodyShape kind={t.body} fill={p.body} />
      <ellipse cx={50} cy={76} rx={16} ry={9} fill="#fff" opacity={0.16} />
      <ellipse cx={36} cy={40} rx={6} ry={3.4} transform="rotate(-32 36 40)" fill="#fff" opacity={0.35} />
      {t.family === 'mask' && <rect x={29} y={49} width={42} height={14} rx={7} fill={p.shade} />}
      <Eye x={50 - eyeGap} kind={t.eyes} />
      <Eye x={50 + eyeGap} kind={t.eyes} />
      {t.blush && (
        <g fill={p.blush} opacity={0.55}>
          <ellipse cx={32} cy={65} rx={5} ry={3} />
          <ellipse cx={68} cy={65} rx={5} ry={3} />
        </g>
      )}
      {t.family === 'tuft' ? <path d="M46.5 64L53.5 64L50 69.5Z" fill="#F2B84B" stroke="#F2B84B" strokeWidth={1.5} strokeLinejoin="round" /> : <MouthShape kind={t.mouth} teeth={t.teeth} />}
    </svg>
  )
}
