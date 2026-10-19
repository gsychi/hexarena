import { StrictMode, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import { Viewer } from './App.tsx'
import { Home } from './Home.tsx'
import { LeaguePage } from './League.tsx'
import { NotebookPage } from './Notebook.tsx'

type Route = { page: 'home' } | { page: 'league' } | { page: 'notes' } | { page: 'match'; id: string }

function parse(hash: string): Route {
  if (/^#\/league\b/.test(hash)) return { page: 'league' }
  if (/^#\/notes\b/.test(hash)) return { page: 'notes' }
  const m = hash.match(/^#\/match\/([A-Za-z0-9]+)/)
  return m ? { page: 'match', id: m[1] } : { page: 'home' }
}

function Root() {
  const [route, setRoute] = useState(() => parse(location.hash))
  const backTo = useRef('/')
  const current = useRef(route)
  useEffect(() => {
    const on = () => {
      const next = parse(location.hash)
      if (next.page === 'match' && current.current.page !== 'match') backTo.current = current.current.page === 'league' ? '/league' : current.current.page === 'notes' ? '/notes' : '/'
      current.current = next
      setRoute(next)
    }
    window.addEventListener('hashchange', on)
    return () => window.removeEventListener('hashchange', on)
  }, [])
  const go = (hash: string) => { location.hash = hash }
  switch (route.page) {
    case 'home':
      return <div className="h-full overflow-y-auto"><Home onOpen={(id) => go(`/match/${id}`)} onLeague={() => go('/league')} onNotes={() => go('/notes')} /></div>
    case 'league':
      return <LeaguePage onBack={() => go('/')} onOpen={(id) => go(`/match/${id}`)} />
    case 'notes':
      return <NotebookPage onBack={() => go('/')} onOpen={(id) => go(`/match/${id}`)} />
    case 'match': {
      const src = route.id === 'sample' ? '/sample-replay.json' : `/api/matches/${route.id}/viewer`
      return <Viewer key={route.id} src={src} label={route.id === 'sample' ? 'Sample match' : `Game ${route.id}`} onBack={() => go(backTo.current)} />
    }
    default: {
      const never: never = route
      return never
    }
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
