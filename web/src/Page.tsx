import { TriangleAlert } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

export function Page({ icon: Icon, title, description, actions, wide, children }: {
  icon: LucideIcon
  title: string
  description: ReactNode
  actions?: ReactNode
  wide?: boolean
  children: ReactNode
}) {
  return (
    <div className={`mx-auto flex min-h-full flex-col gap-6 p-4 sm:p-6 ${wide ? 'max-w-6xl' : 'max-w-4xl'}`}>
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="min-w-0">
          <h1 className="flex items-center gap-2 text-2xl font-semibold tracking-tight"><Icon className="size-5 text-muted-foreground" />{title}</h1>
          <p className="mt-1 max-w-3xl text-sm text-muted-foreground">{description}</p>
        </div>
        {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
      </header>
      {children}
    </div>
  )
}

export function Offline({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <Card className="border-amber-500/40">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base"><TriangleAlert className="size-4 text-amber-400" />Game server isn't reachable</CardTitle>
        <CardDescription>
          Start it with <code className="rounded bg-muted px-1.5 py-0.5 text-foreground">cargo run --release -p hexarena-server</code>, then this page reconnects. You can still watch the sample match.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex gap-2">
        <Button size="sm" onClick={onRetry}>Retry</Button>
        <span className="self-center text-xs text-muted-foreground">{error}</span>
      </CardContent>
    </Card>
  )
}
