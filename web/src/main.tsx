import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import { Viewer } from './App.tsx'
import { Home } from './Home.tsx'

type Route = { page: 'home' } | { page: 'match'; id: string }

function parse(hash: string): Route {
  const m = hash.match(/^#\/match\/([A-Za-z0-9]+)/)
  return m ? { page: 'match', id: m[1] } : { page: 'home' }
}

function Root() {
  const [route, setRoute] = useState(() => parse(location.hash))
  useEffect(() => {
    const on = () => setRoute(parse(location.hash))
    window.addEventListener('hashchange', on)
    return () => window.removeEventListener('hashchange', on)
  }, [])
  const go = (hash: string) => { location.hash = hash }
  if (route.page === 'home') return <div className="h-full overflow-y-auto"><Home onOpen={(id) => go(`/match/${id}`)} /></div>
  const src = route.id === 'sample' ? '/sample-replay.json' : `/api/matches/${route.id}/viewer`
  return <Viewer key={route.id} src={src} label={route.id === 'sample' ? 'Sample match' : `Game ${route.id}`} onBack={() => go('/')} />
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
