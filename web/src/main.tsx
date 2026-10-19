import { BookOpen, BookText, Gamepad2, GitCompareArrows, House, Swords, TrendingUp, Trophy } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { StrictMode, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarHeader, SidebarInset, SidebarMenu,
  SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarRail, SidebarTrigger,
} from '@/components/ui/sidebar'
import './index.css'
import { api } from './api'
import { Viewer } from './App.tsx'
import { BotAvatar } from './BotAvatar.tsx'
import { BotPage } from './BotPage.tsx'
import { ComparePage } from './Compare.tsx'
import { NotebookPage } from './Notebook.tsx'
import { BotsPage, GamesPage, HomePage, ProgressPage } from './Pages.tsx'
import { LiveGame, PlayPage } from './Play.tsx'
import { RulesPage } from './Rules.tsx'

type Section = 'home' | 'play' | 'rules' | 'games' | 'bots' | 'compare' | 'progress' | 'notes'
type Route = { page: Exclude<Section, 'compare'> } | { page: 'compare'; a: string | null; b: string | null } | { page: 'match'; id: string } | { page: 'bot'; id: string } | { page: 'live'; id: string }

const NAV: { page: Section; label: string; icon: LucideIcon; hash: string }[] = [
  { page: 'home', label: 'Home', icon: House, hash: '/' },
  { page: 'play', label: 'Play', icon: Gamepad2, hash: '/play' },
  { page: 'rules', label: 'Rules', icon: BookText, hash: '/rules' },
  { page: 'games', label: 'Games', icon: Swords, hash: '/games' },
  { page: 'bots', label: 'Bots', icon: Trophy, hash: '/bots' },
  { page: 'compare', label: 'Compare', icon: GitCompareArrows, hash: '/compare' },
  { page: 'progress', label: 'Progression', icon: TrendingUp, hash: '/progress' },
  { page: 'notes', label: 'Lab notebook', icon: BookOpen, hash: '/notes' },
]

function parse(hash: string): Route {
  const live = hash.match(/^#\/play\/(p\d+)/)
  if (live) return { page: 'live', id: live[1] }
  const cmp = hash.match(/^#\/compare\b\??(.*)$/)
  if (cmp) {
    const q = new URLSearchParams(cmp[1])
    return { page: 'compare', a: q.get('a'), b: q.get('b') }
  }
  for (const n of NAV) if (n.page !== 'compare' && n.hash !== '/' && new RegExp(`^#${n.hash}\\b`).test(hash)) return { page: n.page }
  const bot = hash.match(/^#\/bot\/([^/?#]+)/)
  if (bot) return { page: 'bot', id: decodeURIComponent(bot[1]) }
  const m = hash.match(/^#\/match\/([A-Za-z0-9]+)/)
  return m ? { page: 'match', id: m[1] } : { page: 'home' }
}

function hashOf(r: Route): string {
  switch (r.page) {
    case 'match': return `/match/${r.id}`
    case 'live': return `/play/${r.id}`
    case 'bot': return `/bot/${encodeURIComponent(r.id)}`
    case 'compare': {
      const q = new URLSearchParams()
      if (r.a) q.set('a', r.a)
      if (r.b) q.set('b', r.b)
      return `/compare${q.size ? `?${q}` : ''}`
    }
    case 'home': case 'play': case 'rules': case 'games': case 'bots': case 'progress': case 'notes': return NAV.find((n) => n.page === r.page)!.hash
    default: { const never: never = r; return never }
  }
}

/** The sidebar entry a route belongs to: a bot's profile sits under Bots, a replay under Games. */
function sectionOf(r: Route): Section {
  switch (r.page) {
    case 'match': return isLiveId(r.id) ? 'play' : 'games'
    case 'live': return 'play'
    case 'bot': return 'bots'
    case 'home': case 'play': case 'rules': case 'games': case 'bots': case 'compare': case 'progress': case 'notes': return r.page
    default: { const never: never = r; return never }
  }
}

/** How often bot lineage is refetched, so new versions' mascots inherit their looks. */
const BOTS_MS = 60_000

const go = (hash: string) => { location.hash = hash }
/** Games a person played live have ids like `p00001` and are served from `/api/play`. */
const isLiveId = (id: string) => /^p\d+$/.test(id)
const openMatch = (id: string) => go(`/match/${id}`)
const openBot = (id: string) => go(`/bot/${encodeURIComponent(id)}`)

function Content({ route, backTo }: { route: Route; backTo: string }) {
  switch (route.page) {
    case 'home': return <div className="h-full overflow-y-auto"><HomePage onBot={openBot} /></div>
    case 'play': return <div className="h-full overflow-y-auto"><PlayPage /></div>
    case 'live': return <LiveGame key={route.id} id={route.id} />
    case 'rules': return <div className="h-full overflow-y-auto"><RulesPage /></div>
    case 'games': return <div className="h-full overflow-y-auto"><GamesPage onOpen={openMatch} /></div>
    case 'bots': return <div className="h-full overflow-y-auto"><BotsPage onBot={openBot} /></div>
    case 'progress': return <div className="h-full overflow-y-auto"><ProgressPage onBot={openBot} /></div>
    case 'notes': return <NotebookPage onOpen={openMatch} />
    case 'compare': return <div className="h-full overflow-y-auto"><ComparePage a={route.a} b={route.b} onBot={openBot} onOpen={openMatch} /></div>
    case 'bot': return <BotPage key={route.id} id={route.id} onBack={() => go('/bots')} onOpen={openMatch} onBot={openBot} />
    case 'match': {
      if (isLiveId(route.id)) return <Viewer key={route.id} src={`/api/play/${route.id}/viewer`} label={`Your game ${route.id}`} onBack={() => go(backTo)} />
      const src = route.id === 'sample' ? '/sample-replay.json' : `/api/matches/${route.id}/viewer`
      return <Viewer key={route.id} src={src} match={route.id === 'sample' ? undefined : route.id} label={route.id === 'sample' ? 'Sample match' : `Game ${route.id}`} onBack={() => go(backTo)} />
    }
    default: { const never: never = route; return never }
  }
}

function Root() {
  const [route, setRoute] = useState(() => parse(location.hash))
  const [open, setOpen] = useState(() => !['match', 'live'].includes(parse(location.hash).page))
  useEffect(() => {
    const load = () => { api.bots().catch(() => {}) }
    load()
    const t = setInterval(load, BOTS_MS)
    return () => clearInterval(t)
  }, [])
  const [backTo, setBackTo] = useState('/games')
  const current = useRef(route)
  useEffect(() => {
    const on = () => {
      const next = parse(location.hash)
      const full = (r: Route) => r.page === 'match' || r.page === 'live'
      if (full(next) && !full(current.current)) {
        setBackTo(hashOf(current.current))
        setOpen(false)
      }
      current.current = next
      setRoute(next)
    }
    window.addEventListener('hashchange', on)
    return () => window.removeEventListener('hashchange', on)
  }, [])
  const section = sectionOf(route)
  const here = NAV.find((n) => n.page === section)!

  return (
    <SidebarProvider open={open} onOpenChange={setOpen}>
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton size="lg" tooltip="Hexarena" render={<a href="#/" />}>
                <BotAvatar id="hexarena" name="Hexarena" size={32} className="shrink-0" />
                <span className="flex min-w-0 flex-col leading-tight group-data-[collapsible=icon]:hidden">
                  <span className="text-[11px] font-semibold tracking-[0.3em]">HEXARENA</span>
                  <span className="truncate text-xs text-sidebar-foreground/60">CPU arena</span>
                </span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu>
                {NAV.map((n) => (
                  <SidebarMenuItem key={n.page}>
                    <SidebarMenuButton isActive={section === n.page} tooltip={n.label} render={<a href={`#${n.hash}`} />}>
                      <n.icon />
                      <span>{n.label}</span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>
        <SidebarFooter className="px-3 pb-3 text-[11px] text-sidebar-foreground/50 group-data-[collapsible=icon]:hidden">
          Bots are written by agents and published to the self-playing arena.
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>
      <SidebarInset className="h-svh min-w-0 overflow-hidden">
        <header className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
          <SidebarTrigger className="-ml-1" />
          <span className="flex items-center gap-1.5 text-sm text-muted-foreground"><here.icon className="size-3.5" />{here.label}</span>
          {route.page === 'bot' && <span className="truncate text-sm text-muted-foreground">/ <span className="font-mono text-xs">{route.id}</span></span>}
          {(route.page === 'match' || route.page === 'live') && <span className="truncate text-sm text-muted-foreground">/ <span className="font-mono text-xs">{route.id}</span></span>}
        </header>
        <div className="min-h-0 flex-1 overflow-hidden"><Content route={route} backTo={backTo} /></div>
      </SidebarInset>
    </SidebarProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
