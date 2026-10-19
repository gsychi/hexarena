import { ArrowLeft, BookOpen, Lightbulb, Loader2, Microscope, Plus, Search, TriangleAlert, Trophy } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { Note, NoteKind } from './api'
import { api } from './api'

const KINDS: { kind: NoteKind; label: string; icon: typeof Lightbulb; tone: string }[] = [
  { kind: 'observation', label: 'Observation', icon: Microscope, tone: 'text-sky-300 border-sky-500/40' },
  { kind: 'lesson', label: 'Lesson', icon: Trophy, tone: 'text-emerald-300 border-emerald-500/40' },
  { kind: 'idea', label: 'Idea', icon: Lightbulb, tone: 'text-amber-300 border-amber-500/40' },
]

function kindInfo(kind: NoteKind) {
  return KINDS.find((k) => k.kind === kind) ?? KINDS[0]
}

function ago(ms: number): string {
  const s = Math.max(0, (Date.now() - ms) / 1000)
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.floor(s / 60)} min ago`
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`
  return new Date(ms).toLocaleDateString()
}

function NoteRow({ note, onOpen }: { note: Note; onOpen: (id: string) => void }) {
  const k = kindInfo(note.kind)
  return (
    <div className="flex flex-col gap-1.5 px-4 py-3">
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <Badge variant="outline" className={`gap-1 ${k.tone}`}><k.icon className="size-3" />{k.label}</Badge>
        <span className="font-medium text-foreground">{note.author.replace(/^agent:/, 'agent ')}</span>
        <span>#{note.n} · {ago(note.at)}</span>
      </div>
      <p className="text-sm leading-relaxed whitespace-pre-wrap">{note.text}</p>
      {(note.bots.length > 0 || note.matches.length > 0 || note.tags.length > 0) && (
        <div className="flex flex-wrap gap-1.5">
          {note.bots.map((b) => <Badge key={`b-${b}`} variant="secondary" className="font-mono text-[11px]">{b}</Badge>)}
          {note.matches.map((m) => (
            <Button key={`m-${m}`} variant="outline" size="xs" className="font-mono" onClick={() => onOpen(m)}>{m}</Button>
          ))}
          {note.tags.map((t) => <span key={`t-${t}`} className="text-[11px] text-muted-foreground">#{t}</span>)}
        </div>
      )}
    </div>
  )
}

export function NotebookPage({ onBack, onOpen }: { onBack: () => void; onOpen: (id: string) => void }) {
  const [notes, setNotes] = useState<Note[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [kind, setKind] = useState<NoteKind | 'all'>('all')
  const [query, setQuery] = useState('')
  const [draft, setDraft] = useState('')
  const [draftKind, setDraftKind] = useState<NoteKind>('observation')
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  const refresh = useCallback(() => {
    api.notes().then((n) => { setNotes(n); setError(null) }).catch((e) => setError(String(e.message ?? e)))
  }, [])

  useEffect(() => {
    refresh()
    const id = setInterval(refresh, 10_000)
    return () => clearInterval(id)
  }, [refresh])

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return (notes ?? []).filter((n) =>
      (kind === 'all' || n.kind === kind) &&
      (q === '' || [n.text, n.author, ...n.bots, ...n.tags].some((s) => s.toLowerCase().includes(q))))
  }, [notes, kind, query])

  const authors = useMemo(() => new Set((notes ?? []).map((n) => n.author)).size, [notes])

  const save = async () => {
    setSaving(true); setSaveError(null)
    try {
      const n = await api.addNote({ author: 'you', kind: draftKind, text: draft })
      setNotes((all) => [n, ...(all ?? [])])
      setDraft('')
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex min-h-full max-w-4xl flex-col gap-6 p-4 sm:p-6">
        <header>
          <Button variant="ghost" size="xs" className="-ml-2 mb-1 text-muted-foreground" onClick={onBack}><ArrowLeft />CPU arena</Button>
          <div className="text-[11px] font-semibold tracking-[0.3em] text-muted-foreground">HEXARENA</div>
          <h1 className="mt-1 flex items-center gap-2 text-2xl font-semibold tracking-tight"><BookOpen className="size-5" />Lab notebook</h1>
          <p className="max-w-2xl text-sm text-muted-foreground">
            What the bot designers have seen in games and learned from results. Agents read it before designing a version and write to it after every gate and replay study; the server logs every arena gate result here too.
            {notes && ` ${notes.length} notes from ${authors} author${authors === 1 ? '' : 's'}.`}
          </p>
        </header>

        <Card size="sm">
          <CardHeader>
            <CardTitle className="text-sm">Add a note</CardTitle>
            <CardDescription>Agents post with <code className="rounded bg-muted px-1 py-0.5 text-foreground">POST /api/observations</code>.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            <Textarea value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="e.g. Snowshoe's walls only go up after its border is already lost. Seen in m01020." rows={3} />
            <div className="flex flex-wrap items-center justify-between gap-2">
              <ToggleGroup variant="outline" size="sm" value={[draftKind]} onValueChange={(v) => v.length && setDraftKind(v[v.length - 1] as NoteKind)}>
                {KINDS.map((k) => <ToggleGroupItem key={k.kind} value={k.kind} className="data-pressed:bg-foreground data-pressed:text-background">{k.label}</ToggleGroupItem>)}
              </ToggleGroup>
              <Button size="sm" onClick={save} disabled={saving || draft.trim() === ''}>{saving ? <Loader2 className="animate-spin" /> : <Plus />}Add note</Button>
            </div>
            {saveError && <p className="text-xs text-red-400">Couldn't save: {saveError}</p>}
          </CardContent>
        </Card>

        <div className="flex flex-wrap items-center gap-2">
          <ToggleGroup variant="outline" size="sm" value={[kind]} onValueChange={(v) => v.length && setKind(v[v.length - 1] as NoteKind | 'all')}>
            <ToggleGroupItem value="all" className="data-pressed:bg-foreground data-pressed:text-background">All</ToggleGroupItem>
            {KINDS.map((k) => <ToggleGroupItem key={k.kind} value={k.kind} className="data-pressed:bg-foreground data-pressed:text-background">{k.label}s</ToggleGroupItem>)}
          </ToggleGroup>
          <div className="relative min-w-48 flex-1">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search text, bots, authors, tags" className="h-8 pl-8" />
          </div>
        </div>

        {error && !notes && (
          <Card className="border-amber-500/40">
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-base"><TriangleAlert className="size-4 text-amber-400" />Can't load the notebook</CardTitle>
              <CardDescription>{error}</CardDescription>
            </CardHeader>
            <CardContent><Button size="sm" onClick={refresh}>Retry</Button></CardContent>
          </Card>
        )}
        {!notes && !error && (
          <div className="flex items-center justify-center gap-2 py-16 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading notes…</div>
        )}
        {notes && shown.length === 0 && (
          <p className="py-10 text-center text-sm text-muted-foreground">{notes.length === 0 ? 'No notes yet. The first gate result will show up here.' : 'No notes match.'}</p>
        )}
        {shown.length > 0 && (
          <Card className="py-0">
            <div className="divide-y">
              {shown.map((n) => <NoteRow key={n.n} note={n} onOpen={onOpen} />)}
            </div>
          </Card>
        )}
      </div>
    </div>
  )
}
