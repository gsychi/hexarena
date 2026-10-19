import { Clapperboard, Move3d } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js'
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js'
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js'
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js'
import { Button } from '@/components/ui/button'
import type { Cell, GameEvent, Hex, ViewerReplay } from './model'
import { hexAt, hexIndex, PLAYER_COLORS } from './model'

const SQ3 = Math.sqrt(3)
const worldX = (h: Hex) => SQ3 * (h.q + h.r / 2)
const worldZ = (h: Hex) => 1.5 * h.r

const TERRAIN: Record<number, string> = { [-2]: '#2c3658', [-1]: '#3a4f6b', [0]: '#3f5f3e', [1]: '#6f6650', [2]: '#866a52' }
const tileTop = (height: number) => 0.55 + (height + 2) * 0.22
const WATER_TOP = 0.22
const MOUNTAIN_TOP = 1.75

/** Recent ticks the director camera looks back over to find where the fighting is. */
const HOT_TICKS = 40
const FLASHES = 320
/** Ticks the board may jump forward and still spark every capture in between. */
const FLASH_JUMP = 6

type CameraMode = 'cinematic' | 'free'

interface Flash { idx: number; color: THREE.Color; born: number; life: number; size: number }

interface World {
  renderer: THREE.WebGLRenderer
  composer: EffectComposer
  bloom: UnrealBloomPass
  camera: THREE.PerspectiveCamera
  controls: OrbitControls
  tiles: THREE.InstancedMesh
  pillars: THREE.InstancedMesh
  farms: THREE.InstancedMesh
  houses: THREE.InstancedMesh
  roofs: THREE.InstancedMesh
  quarries: THREE.InstancedMesh
  towers: THREE.InstancedMesh
  rings: THREE.InstancedMesh
  /** Instance slot of every passable hex, by hex index (-1 off board or impassable). */
  slot: Int32Array
  hexes: number[]
  top: Float32Array
  pillar: Float32Array
  pillarTarget: Float32Array
  pillarOffset: Float32Array
  flashes: Flash[]
  focus: THREE.Vector3
  focusTarget: THREE.Vector3
  heat: number
  heatTarget: number
  span: number
  mode: CameraMode
  dispose: () => void
}

function hash(i: number) {
  const x = Math.sin(i * 127.1 + 311.7) * 43758.5453
  return x - Math.floor(x)
}

/** The hexes an event happens on (none for board-wide events). */
function eventHexes(e: GameEvent): Hex[] {
  switch (e.type) {
    case 'captured': return [e.at]
    case 'repelled': return [e.at]
    case 'surrendered': return e.hexes
    case 'built': return [e.at]
    case 'demolished': return [e.at]
    case 'moved': case 'gathered': case 'link_changed': case 'links_broken': case 'eliminated': case 'rejected': return []
    default: { const never: never = e; return never }
  }
}

/** The biggest thing that happened in the last few seconds, as a one-line caption. */
function headline(rep: ViewerReplay, tick: number, visible: Set<number> | null): string | null {
  const best = { score: -1, text: null as string | null }
  const consider = (score: number, text: string) => { if (score > best.score) { best.score = score; best.text = text } }
  for (let t = tick; t >= Math.max(1, tick - 60); t--) {
    const fresh = 1 - (tick - t) / 120
    for (const e of rep.events[t] ?? []) {
      const at = eventHexes(e)
      if (visible && at.length > 0 && !at.some((h) => visible.has(hexIndex(rep.radius, h)))) continue
      switch (e.type) {
        case 'eliminated': consider(1000 * fresh, `${rep.names[e.player]} is eliminated`); break
        case 'surrendered': if (e.hexes.length >= 2) consider((40 + e.hexes.length * 8) * fresh, `${rep.names[e.capturer]} surrounds ${e.hexes.length} hexes of ${rep.names[e.from]}`); break
        case 'captured': if (e.previous_owner !== null && e.building) consider(30 * fresh, `${rep.names[e.player]} seizes a ${e.building} from ${rep.names[e.previous_owner]}`); break
        case 'demolished': consider(20 * fresh, `${rep.names[e.player]} tears down a ${e.building}`); break
        case 'moved': case 'gathered': case 'repelled': case 'built': case 'link_changed': case 'links_broken': case 'rejected': break
        default: { const never: never = e; return never }
      }
    }
  }
  return best.text
}

function buildWorld(container: HTMLDivElement, rep: ViewerReplay): World {
  const renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: 'high-performance' })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.75))
  renderer.shadowMap.enabled = true
  renderer.shadowMap.type = THREE.PCFSoftShadowMap
  renderer.toneMapping = THREE.ACESFilmicToneMapping
  renderer.toneMappingExposure = 1.05
  container.appendChild(renderer.domElement)
  renderer.domElement.style.display = 'block'

  const scene = new THREE.Scene()
  scene.background = new THREE.Color('#05060b')
  const span = SQ3 * rep.radius + 1.5
  scene.fog = new THREE.Fog('#05060b', span * 1.6, span * 4.2)

  const camera = new THREE.PerspectiveCamera(38, 1, 0.1, span * 12)
  camera.position.set(0, span * 1.3, span * 1.7)

  scene.add(new THREE.HemisphereLight('#a9b8ff', '#1b1022', 0.75))
  const key = new THREE.DirectionalLight('#ffd7a3', 2.1)
  key.position.set(-span * 0.8, span * 1.4, span * 0.6)
  key.castShadow = true
  key.shadow.mapSize.set(2048, 2048)
  const sc = key.shadow.camera
  sc.left = -span * 1.2; sc.right = span * 1.2; sc.top = span * 1.2; sc.bottom = -span * 1.2
  sc.near = 1; sc.far = span * 5
  key.shadow.bias = -0.0006
  key.shadow.normalBias = 0.02
  scene.add(key)
  const rim = new THREE.DirectionalLight('#5f7dff', 0.9)
  rim.position.set(span, span * 0.5, -span)
  scene.add(rim)

  const plinth = new THREE.Mesh(
    new THREE.CylinderGeometry(span + 1.2, span + 2.2, 1.2, 6),
    new THREE.MeshStandardMaterial({ color: '#0d0f18', roughness: 0.6, metalness: 0.3 }),
  )
  plinth.rotation.y = Math.PI / 6
  plinth.position.y = -0.6
  plinth.receiveShadow = true
  scene.add(plinth)
  const floor = new THREE.Mesh(new THREE.CircleGeometry(span * 8, 64), new THREE.MeshStandardMaterial({ color: '#07080e', roughness: 1 }))
  floor.rotation.x = -Math.PI / 2
  floor.position.y = -1.2
  floor.receiveShadow = true
  scene.add(floor)

  const size = (2 * rep.radius + 1) ** 2
  const hexes: number[] = []
  const water: number[] = []
  const rock: number[] = []
  for (let i = 0; i < size; i++) {
    const t = rep.terrain[i]
    if (!t) continue
    if (t.kind === 'open') hexes.push(i)
    else if (t.kind === 'water') water.push(i)
    else rock.push(i)
  }
  const slot = new Int32Array(size).fill(-1)
  hexes.forEach((h, s) => { slot[h] = s })

  const prism = new THREE.CylinderGeometry(0.97, 0.97, 1, 6)
  prism.translate(0, 0.5, 0)
  const m = new THREE.Matrix4()
  const q = new THREE.Quaternion()
  const v = new THREE.Vector3()
  const sv = new THREE.Vector3()
  const place = (mesh: THREE.InstancedMesh, i: number, x: number, y: number, z: number, sx: number, sy: number, sz: number, rot = 0) => {
    q.setFromAxisAngle(THREE.Object3D.DEFAULT_UP, rot)
    mesh.setMatrixAt(i, m.compose(v.set(x, y, z), q, sv.set(sx, sy, sz)))
  }
  const instanced = (geo: THREE.BufferGeometry, mat: THREE.Material, n: number, shadows = true) => {
    const mesh = new THREE.InstancedMesh(geo, mat, Math.max(1, n))
    mesh.count = n
    mesh.castShadow = shadows
    mesh.receiveShadow = shadows
    mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage)
    scene.add(mesh)
    return mesh
  }

  const top = new Float32Array(size)
  const tiles = instanced(prism, new THREE.MeshStandardMaterial({ roughness: 0.82, metalness: 0.04 }), hexes.length)
  const c = new THREE.Color()
  hexes.forEach((h, s) => {
    const p = hexAt(rep.radius, h)
    const height = rep.terrain[h]!.height
    top[h] = tileTop(height)
    place(tiles, s, worldX(p), 0, worldZ(p), 1, top[h], 1)
    tiles.setColorAt(s, c.set(TERRAIN[height] ?? TERRAIN[0]))
  })

  const sea = instanced(prism, new THREE.MeshStandardMaterial({ color: '#174a86', roughness: 0.18, metalness: 0.35, emissive: '#0a2a55', emissiveIntensity: 0.6 }), water.length, false)
  sea.receiveShadow = true
  water.forEach((h, s) => { const p = hexAt(rep.radius, h); top[h] = WATER_TOP; place(sea, s, worldX(p), 0, worldZ(p), 1, WATER_TOP, 1) })
  const crags = instanced(prism, new THREE.MeshStandardMaterial({ color: '#4c505c', roughness: 0.95 }), rock.length)
  const peakGeo = new THREE.ConeGeometry(0.9, 1, 6)
  peakGeo.translate(0, 0.5, 0)
  const peaks = instanced(peakGeo, new THREE.MeshStandardMaterial({ color: '#c5c9d6', roughness: 0.7 }), rock.length)
  rock.forEach((h, s) => {
    const p = hexAt(rep.radius, h)
    const tall = MOUNTAIN_TOP + hash(h) * 0.9
    top[h] = tall
    place(crags, s, worldX(p), 0, worldZ(p), 1, tall, 1)
    place(peaks, s, worldX(p), tall, worldZ(p), 0.95, 0.8 + hash(h + 7) * 1.1, 0.95, hash(h + 3) * Math.PI)
  })

  const n = hexes.length
  const pillarGeo = new THREE.CylinderGeometry(0.18, 0.24, 1, 6)
  pillarGeo.translate(0, 0.5, 0)
  const pillars = instanced(pillarGeo, new THREE.MeshBasicMaterial({ toneMapped: false }), n, false)
  pillars.castShadow = true
  const farmGeo = new THREE.CylinderGeometry(0.78, 0.78, 0.07, 6)
  farmGeo.translate(0, 0.035, 0)
  const farms = instanced(farmGeo, new THREE.MeshStandardMaterial({ color: '#d4a63a', roughness: 0.9, emissive: '#3a2a05', emissiveIntensity: 0.5 }), n)
  const houseGeo = new THREE.BoxGeometry(0.46, 0.32, 0.46)
  houseGeo.translate(0, 0.16, 0)
  const houses = instanced(houseGeo, new THREE.MeshStandardMaterial({ color: '#e9dcc4', roughness: 0.8, emissive: '#ff9d3b', emissiveIntensity: 0.35 }), n)
  const roofGeo = new THREE.ConeGeometry(0.42, 0.3, 4)
  roofGeo.rotateY(Math.PI / 4)
  roofGeo.translate(0, 0.32 + 0.15, 0)
  const roofs = instanced(roofGeo, new THREE.MeshStandardMaterial({ roughness: 0.6 }), n)
  const quarries = instanced(new THREE.DodecahedronGeometry(0.3), new THREE.MeshStandardMaterial({ color: '#9aa0ad', roughness: 0.95, flatShading: true }), n)
  const towerGeo = new THREE.CylinderGeometry(0.26, 0.32, 0.72, 8)
  towerGeo.translate(0, 0.36, 0)
  const towers = instanced(towerGeo, new THREE.MeshStandardMaterial({ roughness: 0.7, metalness: 0.15 }), n)

  const ringGeo = new THREE.RingGeometry(0.55, 0.95, 6, 1, Math.PI / 2)
  ringGeo.rotateX(-Math.PI / 2)
  const rings = instanced(ringGeo, new THREE.MeshBasicMaterial({ transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide, toneMapped: false }), FLASHES, false)
  for (let i = 0; i < FLASHES; i++) { place(rings, i, 0, -10, 0, 0, 0, 0); rings.setColorAt(i, c.set(0, 0, 0)) }
  for (const mesh of [pillars, farms, houses, roofs, quarries, towers]) {
    for (let i = 0; i < n; i++) { place(mesh, i, 0, -10, 0, 0, 0, 0); mesh.setColorAt(i, c.set('#ffffff')) }
  }

  const composer = new EffectComposer(renderer)
  composer.addPass(new RenderPass(scene, camera))
  const bloom = new UnrealBloomPass(new THREE.Vector2(256, 256), 0.6, 0.5, 0.85)
  composer.addPass(bloom)
  composer.addPass(new OutputPass())

  const controls = new OrbitControls(camera, renderer.domElement)
  controls.enableDamping = true
  controls.maxPolarAngle = Math.PI * 0.47
  controls.minDistance = 6
  controls.maxDistance = span * 4
  controls.enabled = false

  return {
    renderer, composer, bloom, camera, controls, tiles, pillars, farms, houses, roofs, quarries, towers, rings, slot, hexes, top,
    pillar: new Float32Array(n), pillarTarget: new Float32Array(n), pillarOffset: new Float32Array(n),
    flashes: [], focus: new THREE.Vector3(), focusTarget: new THREE.Vector3(), heat: 0, heatTarget: 0, span, mode: 'cinematic',
    dispose: () => {
      controls.dispose()
      scene.traverse((o) => {
        const mesh = o as THREE.Mesh
        mesh.geometry?.dispose()
        const mat = mesh.material as THREE.Material | THREE.Material[] | undefined
        if (Array.isArray(mat)) mat.forEach((x) => x.dispose())
        else mat?.dispose()
      })
      composer.dispose()
      renderer.dispose()
      renderer.domElement.remove()
    },
  }
}

const pillarHeight = (troops: number) => (troops > 0 ? 0.12 + 0.38 * Math.log10(1 + troops) : 0)

/** Writes one tick's ownership, troops and buildings into the instance buffers. */
function paint(w: World, rep: ViewerReplay, cells: Map<number, Cell>, visible: Set<number> | null) {
  const c = new THREE.Color()
  const owner = new THREE.Color()
  const m = new THREE.Matrix4()
  const q = new THREE.Quaternion()
  const v = new THREE.Vector3()
  const s = new THREE.Vector3()
  const hide = (mesh: THREE.InstancedMesh, i: number) => mesh.setMatrixAt(i, m.makeScale(0, 0, 0))
  const players = PLAYER_COLORS.map((x) => new THREE.Color(x))
  w.hexes.forEach((idx, i) => {
    const h = hexAt(rep.radius, idx)
    const x = worldX(h), z = worldZ(h), y = w.top[idx]
    const seen = !visible || visible.has(idx)
    const cell = seen ? cells.get(idx) : undefined
    const base = c.set(TERRAIN[rep.terrain[idx]!.height] ?? TERRAIN[0])
    const own = cell && cell.owner >= 0 ? cell.owner : -1
    if (own >= 0) base.lerp(owner.copy(players[own % 8]), 0.62)
    if (!seen) base.multiplyScalar(0.28)
    w.tiles.setColorAt(i, base)

    const b = cell?.building ?? 0
    w.pillarTarget[i] = cell ? pillarHeight(cell.troops) * (own >= 0 ? 1 : 0.55) : 0
    const offset = b > 0 ? 0.42 : 0
    if (offset !== w.pillarOffset[i]) { w.pillarOffset[i] = offset; w.pillar[i] += 0.01 }
    const glow = own >= 0 ? owner.copy(players[own % 8]).multiplyScalar(1.3) : owner.set('#7d8292').multiplyScalar(0.55)
    w.pillars.setColorAt(i, glow)

    const bx = b > 0 ? x - 0.14 : x
    q.identity()
    for (const [kind, mesh] of [[1, w.farms], [2, w.houses], [3, w.quarries], [4, w.towers]] as const) {
      if (b === kind) mesh.setMatrixAt(i, m.compose(v.set(kind === 1 ? x : bx, y + (kind === 3 ? 0.22 : 0), z), q, s.set(1, 1, 1)))
      else hide(mesh, i)
    }
    if (b === 2) {
      w.roofs.setMatrixAt(i, m.compose(v.set(bx, y, z), q, s.set(1, 1, 1)))
      w.roofs.setColorAt(i, own >= 0 ? owner.copy(players[own % 8]) : owner.set('#8a8f99'))
    } else hide(w.roofs, i)
    if (b === 4) w.towers.setColorAt(i, own >= 0 ? owner.set('#b9bcc6').lerp(players[own % 8], 0.35) : owner.set('#9a9da6'))
  })
  for (const mesh of [w.tiles, w.pillars, w.farms, w.houses, w.roofs, w.quarries, w.towers]) {
    mesh.instanceMatrix.needsUpdate = true
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true
  }
}

/** Sparks a ring on every capture and surrender between two ticks. */
function spark(w: World, rep: ViewerReplay, from: number, to: number, visible: Set<number> | null, now: number) {
  for (let t = from + 1; t <= to; t++) {
    for (const e of rep.events[t] ?? []) {
      let color: string | null = null, life = 900, size = 1
      if (e.type === 'captured') { color = PLAYER_COLORS[e.player % 8]; if (e.previous_owner === null) { life = 650; size = 0.75 } }
      else if (e.type === 'surrendered') { color = PLAYER_COLORS[e.capturer % 8]; life = 1700; size = 1.35 }
      else if (e.type === 'repelled') { color = '#ffffff'; life = 500; size = 0.6 }
      if (!color) continue
      for (const h of eventHexes(e)) {
        const idx = hexIndex(rep.radius, h)
        if (visible && !visible.has(idx)) continue
        if (w.flashes.length >= FLASHES) w.flashes.shift()
        w.flashes.push({ idx, color: new THREE.Color(color), born: now, life, size })
      }
    }
  }
}

/** Where the fighting is: the centre of recent captures, and how intense it is. */
function hotspot(rep: ViewerReplay, tick: number, visible: Set<number> | null): { x: number; z: number; heat: number } {
  let x = 0, z = 0, wsum = 0
  for (let t = tick; t >= Math.max(1, tick - HOT_TICKS); t--) {
    const fade = 1 - (tick - t) / (HOT_TICKS + 1)
    for (const e of rep.events[t] ?? []) {
      const weight = e.type === 'surrendered' ? 3 : e.type === 'captured' ? (e.previous_owner === null ? 0.3 : 1) : 0
      if (!weight) continue
      for (const h of eventHexes(e)) {
        if (visible && !visible.has(hexIndex(rep.radius, h))) continue
        x += worldX(h) * weight * fade; z += worldZ(h) * weight * fade; wsum += weight * fade
      }
    }
  }
  return wsum > 0 ? { x: x / wsum, z: z / wsum, heat: Math.min(1, wsum / 25) } : { x: 0, z: 0, heat: 0 }
}

export function Board3D({ rep, cells, tick, visible }: { rep: ViewerReplay; cells: Map<number, Cell>; tick: number; visible: Set<number> | null }) {
  const host = useRef<HTMLDivElement>(null)
  const world = useRef<World | null>(null)
  const lastTick = useRef(tick)
  const [mode, setMode] = useState<CameraMode>('cinematic')
  const [failed, setFailed] = useState<string | null>(null)

  useEffect(() => {
    const el = host.current
    if (!el) return
    let w: World
    try {
      w = buildWorld(el, rep)
    } catch (e) {
      setFailed(String((e as Error).message ?? e))
      return
    }
    world.current = w
    const resize = () => {
      const { clientWidth: cw, clientHeight: ch } = el
      if (!cw || !ch) return
      w.renderer.setSize(cw, ch)
      w.composer.setSize(cw, ch)
      w.bloom.resolution.set(cw / 2, ch / 2)
      w.camera.aspect = cw / ch
      w.camera.updateProjectionMatrix()
    }
    resize()
    const ro = new ResizeObserver(resize)
    ro.observe(el)

    const clock = new THREE.Clock()
    const m = new THREE.Matrix4()
    const q = new THREE.Quaternion()
    const v = new THREE.Vector3()
    const s = new THREE.Vector3()
    const black = new THREE.Color(0, 0, 0)
    const fc = new THREE.Color()
    const lookAt = new THREE.Vector3()
    const want = new THREE.Vector3()
    let orbit = 0.6
    let frame = 0
    const loop = () => {
      frame = requestAnimationFrame(loop)
      const dt = Math.min(0.05, clock.getDelta())
      const now = performance.now()
      const ease = 1 - Math.exp(-dt * 7)

      let moved = false
      for (let i = 0; i < w.hexes.length; i++) {
        const d = w.pillarTarget[i] - w.pillar[i]
        if (Math.abs(d) < 1e-3) continue
        w.pillar[i] += d * ease
        const idx = w.hexes[i]
        const h = hexAt(rep.radius, idx)
        const ph = w.pillar[i]
        w.pillars.setMatrixAt(i, ph > 0.01 ? m.compose(v.set(worldX(h) + w.pillarOffset[i], w.top[idx], worldZ(h)), q.identity(), s.set(1, ph, 1)) : m.makeScale(0, 0, 0))
        moved = true
      }
      if (moved) w.pillars.instanceMatrix.needsUpdate = true

      w.flashes = w.flashes.filter((f) => now - f.born < f.life)
      for (let i = 0; i < FLASHES; i++) {
        const f = w.flashes[i]
        if (!f) { w.rings.setMatrixAt(i, m.makeScale(0, 0, 0)); w.rings.setColorAt(i, black); continue }
        const a = (now - f.born) / f.life
        const h = hexAt(rep.radius, f.idx)
        const r = f.size * (0.55 + 1.15 * a)
        w.rings.setMatrixAt(i, m.compose(v.set(worldX(h), w.top[f.idx] + 0.06 + a * 0.5, worldZ(h)), q.identity(), s.set(r, 1, r)))
        w.rings.setColorAt(i, fc.copy(f.color).multiplyScalar(2.6 * (1 - a) ** 1.6))
      }
      w.rings.instanceMatrix.needsUpdate = true
      if (w.rings.instanceColor) w.rings.instanceColor.needsUpdate = true

      if (w.mode === 'cinematic') {
        w.focus.lerp(w.focusTarget, 1 - Math.exp(-dt * 0.9))
        w.heat += (w.heatTarget - w.heat) * (1 - Math.exp(-dt * 0.8))
        orbit += dt * (0.045 + w.heat * 0.03)
        const dist = w.span * (2.05 - w.heat * 0.7)
        const pitch = 0.62 + Math.sin(now / 9000) * 0.08 - w.heat * 0.1
        want.set(
          w.focus.x + Math.cos(orbit) * Math.cos(pitch) * dist,
          Math.sin(pitch) * dist + 1,
          w.focus.z + Math.sin(orbit) * Math.cos(pitch) * dist,
        )
        want.x += Math.sin(now / 2300) * 0.25
        want.y += Math.sin(now / 3100) * 0.18
        w.camera.position.lerp(want, 1 - Math.exp(-dt * 1.6))
        lookAt.lerp(v.set(w.focus.x, 0.8, w.focus.z), 1 - Math.exp(-dt * 2.2))
        w.camera.lookAt(lookAt)
      } else {
        w.controls.update()
        lookAt.copy(w.controls.target)
      }
      w.composer.render(dt)
    }
    loop()
    return () => {
      cancelAnimationFrame(frame)
      ro.disconnect()
      w.dispose()
      world.current = null
    }
  }, [rep])

  useEffect(() => {
    const w = world.current
    if (!w) return
    paint(w, rep, cells, visible)
    const prev = lastTick.current
    if (tick > prev && tick - prev <= FLASH_JUMP) spark(w, rep, prev, tick, visible, performance.now())
    lastTick.current = tick
    const hot = hotspot(rep, tick, visible)
    w.focusTarget.set(hot.x * (0.35 + 0.55 * hot.heat), 0, hot.z * (0.35 + 0.55 * hot.heat))
    w.heatTarget = hot.heat
  }, [rep, cells, visible, tick])

  useEffect(() => {
    const w = world.current
    if (!w) return
    w.mode = mode
    w.controls.enabled = mode === 'free'
    if (mode === 'free') {
      w.controls.target.set(w.focus.x, 0.8, w.focus.z)
      w.controls.update()
    }
  }, [mode])

  const caption = useMemo(() => headline(rep, tick, visible), [rep, tick, visible])
  const land = useMemo(() => {
    const counts = rep.names.map(() => 0)
    for (const [idx, c] of cells) if (c.owner >= 0 && (!visible || visible.has(idx))) counts[c.owner]++
    const total = counts.reduce((a, b) => a + b, 0)
    return { counts, total }
  }, [rep, cells, visible])
  const cinematic = mode === 'cinematic'

  if (failed) return (
    <div className="flex h-full items-center justify-center rounded-xl border bg-card p-6 text-center text-sm text-muted-foreground">
      The 3D view needs WebGL, which this browser could not start ({failed}). Switch back to the 2D board.
    </div>
  )

  return (
    <div className="relative h-full overflow-hidden rounded-xl border bg-[#05060b]">
      <div ref={host} className="absolute inset-0" />
      <div className="pointer-events-none absolute inset-0" style={{ background: 'radial-gradient(ellipse at center, transparent 55%, rgba(0,0,0,0.6) 100%)' }} />
      <div className={`pointer-events-none absolute inset-x-0 top-0 bg-black transition-all duration-700 ${cinematic ? 'h-[8%]' : 'h-0'}`} />
      <div className={`pointer-events-none absolute inset-x-0 bottom-0 bg-black transition-all duration-700 ${cinematic ? 'h-[11%]' : 'h-0'}`} />

      <div className="absolute inset-x-0 top-0 flex items-start justify-between gap-3 p-3">
        <div className="pointer-events-none w-[min(22rem,55%)]">
          <div className="text-[10px] font-medium uppercase tracking-[0.3em] text-white/60">Tick {tick.toLocaleString()}</div>
          {land.total > 0 && (
            <div className="mt-1.5 flex h-1.5 overflow-hidden rounded-full bg-white/10">
              {land.counts.map((n, p) => n > 0 && <div key={p} style={{ width: `${(n / land.total) * 100}%`, background: PLAYER_COLORS[p % 8] }} title={`${rep.names[p]}: ${n} hexes`} />)}
            </div>
          )}
        </div>
        <div className="flex gap-1">
          <Button size="xs" variant={cinematic ? 'default' : 'secondary'} onClick={() => setMode('cinematic')} title="The camera follows the fighting"><Clapperboard />Director</Button>
          <Button size="xs" variant={cinematic ? 'secondary' : 'default'} onClick={() => setMode('free')} title="Drag to orbit, scroll to zoom, right-drag to pan"><Move3d />Free camera</Button>
        </div>
      </div>

      <div className="pointer-events-none absolute inset-x-0 bottom-0 flex h-[11%] min-h-10 items-center justify-center px-6">
        {caption && (
          <div key={caption} className="animate-in fade-in slide-in-from-bottom-1 text-center text-sm font-medium tracking-wide text-white/90 duration-700 sm:text-base">
            {caption}
          </div>
        )}
      </div>
      {!cinematic && <div className="pointer-events-none absolute bottom-3 left-3 text-[11px] text-white/50">Drag to orbit · scroll to zoom · right-drag to pan</div>}
    </div>
  )
}
