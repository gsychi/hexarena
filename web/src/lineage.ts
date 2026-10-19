import { useSyncExternalStore } from 'react'
import type { BotInfo } from './api'

/** Who each bot's parents are, so a mascot can inherit its parents' looks. */
const lineage = new Map<string, { name: string; parents: string[] }>()
const listeners = new Set<() => void>()
let version = 0

export function registerBots(bots: BotInfo[]) {
  for (const b of bots) lineage.set(b.id, { name: b.name, parents: b.parents ?? [] })
  version++
  for (const l of listeners) l()
}

export function lineageOf(id: string) {
  return lineage.get(id)
}

function subscribe(l: () => void) {
  listeners.add(l)
  return () => { listeners.delete(l) }
}

/** Re-renders the caller whenever new lineage arrives. */
export function useLineageVersion() {
  return useSyncExternalStore(subscribe, () => version)
}
