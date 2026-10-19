import { Bot, FlaskConical, GraduationCap, Hammer, Lightbulb, Loader2, MessageCircle, Microscope, Send, Sparkles } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Textarea } from '@/components/ui/textarea'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { api } from './api'
import type { Activity, ActivityItem, Agent, AgentStatus, ArenaEntrant, NoteKind } from './api'
import { AgentAvatar } from './AgentAvatar'
import { BotAvatar } from './BotAvatar'
import { usePoll } from './poll'

const AGENTS_MS = 10_000
const ACTIVITY_MS = 5000
/** Seen within this long counts as active now. */
const ACTIVE_MS = 20 * 60_000
/** A test that started this recently and hasn't reported a result is shown as running. */
const RUNNING_MS = 45 * 60_000
const ARRIVALS = 8
const QUICK = ['Hi!', 'What are you working on?', 'What did you learn from the last replays?']

type Filter = 'all' | 'bot' | 'work' | 'note' | 'message'
const FILTERS: { value: Filter; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'bot', label: 'Bots' },
  { value: 'work', label: 'Tests' },
  { value: 'note', label: 'Notes' },
  { value: 'message', label: 'Chat' },
]

const NOTE_KINDS: Record<NoteKind, { label: string; icon: LucideIcon; tone: string }> = {
  observation: { label: 'Observation', icon: Microscope, tone: 'text-sky-300' },
  lesson: { label: 'Lesson', icon: GraduationCap, tone: 'text-emerald-300' },
  idea: { label: 'Idea', icon: Lightbulb, tone: 'text-amber-300' },
}

const loadAgents = () => Promise.all([api.agents(), api.arena()])

function ago(ms: number) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.round(s / 60)}m ago`
  if (s < 86400) return `${Math.round(s / 3600)}h ago`
  return `${Math.round(s / 86400)}d ago`
}

/** `agent:bunny-sol` → `Bunny Sol`. */
export function agentName(id: string) {
  return id.replace(/^agent:/, '').split(/[-_]/).map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(' ')
}

function Status({ at }: { at: number }) {
  const live = at > 0 && Date.now() - at < ACTIVE_MS
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs ${live ? 'text-emerald-400' : 'text-muted-foreground'}`}>
      <span className="relative flex size-2">
        {live && <span className="absolute inline-flex size-full animate-ping rounded-full bg-emerald-400 opacity-60" />}
        <span className={`relative inline-flex size-2 rounded-full ${live ? 'bg-emerald-400' : 'bg-muted-foreground/50'}`} />
      </span>
      {live ? `active ${ago(at)}` : at ? `last seen ${ago(at)}` : 'not seen yet'}
    </span>
  )
}

/** The agent's own status if it set one, otherwise its newest note or bot as a stand-in. */
function WorkingOn({ a }: { a: Agent }) {
  const fallback = a.last_note && (!a.latest_bot || a.last_note.at >= a.latest_bot.at)
    ? { label: 'Last note', text: a.last_note.text, at: a.last_note.at }
    : a.latest_bot ? { label: 'Last sent in', text: a.latest_bot.name, at: a.latest_bot.at } : null
  const shown = a.status ? { label: 'Working on', text: a.status.text, at: a.status.at } : fallback
  if (!shown) return null
  return (
    <div className={`rounded-lg border px-3 py-2 ${a.status ? 'border-emerald-500/30 bg-emerald-500/5' : 'bg-muted/20'}`}>
      <div className="flex items-center justify-between gap-2 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
        <span className={`flex items-center gap-1 ${a.status ? 'text-emerald-300' : ''}`}><Hammer className="size-3" />{shown.label}</span>
        <span className="normal-case tracking-normal">{ago(shown.at)}</span>
      </div>
      <p className="mt-1 line-clamp-3 text-xs text-foreground/90" title={shown.text}>{shown.text}</p>
    </div>
  )
}

/** What the agent's tools are doing right now, from the latest eval report. */
function Doing({ d }: { d: AgentStatus }) {
  const running = d.text.startsWith('Testing ') && Date.now() - d.at < RUNNING_MS
  return (
    <div className="flex items-start gap-2 text-xs">
      {running ? <Loader2 className="mt-0.5 size-3.5 shrink-0 animate-spin text-sky-300" /> : <FlaskConical className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />}
      <span className={running ? 'text-sky-200' : 'text-muted-foreground'}>{d.text} <span className="text-muted-foreground">({ago(d.at)})</span></span>
    </div>
  )
}

function AgentCard({ a, onBot, onOpen }: { a: Agent; onBot: (id: string) => void; onOpen: () => void }) {
  const replied = a.last_message?.from === a.id
  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-muted/15 p-4">
      <div className="flex items-start gap-3">
        <AgentAvatar id={a.id} model={a.model} size={52} className="shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold">{agentName(a.id)}</span>
            {a.model && <Badge variant="outline" className="text-[10px]">{a.model}</Badge>}
          </div>
          <Status at={a.last_seen} />
        </div>
      </div>
      <WorkingOn a={a} />
      {a.doing && <Doing d={a.doing} />}
      <div className="space-y-1 text-xs text-muted-foreground">
        <div><span className="tabular-nums text-foreground">{a.bots}</span> bot{a.bots === 1 ? '' : 's'} sent in, <span className="tabular-nums text-foreground">{a.active_bots}</span> in the arena now, <span className="tabular-nums text-foreground">{a.notes}</span> note{a.notes === 1 ? '' : 's'}</div>
        {a.best_bot && (
          <div>
            Best: <button type="button" className="cursor-pointer text-foreground hover:underline" onClick={() => onBot(a.best_bot!.id)}>{a.best_bot.name}</button>
            <span className="tabular-nums">, #{a.best_bot.rank} at {Math.round(a.best_bot.elo)}</span>
          </div>
        )}
      </div>
      <div className="mt-auto flex flex-wrap items-center gap-2">
        <Button size="sm" variant={replied ? 'default' : 'outline'} onClick={onOpen}>
          {replied ? <><MessageCircle />New reply</> : <><Sparkles />Activity and chat</>}
        </Button>
        {a.unanswered > 0 && !replied && <span className="text-[11px] text-muted-foreground">{a.unanswered} message{a.unanswered === 1 ? '' : 's'} waiting for a reply</span>}
      </div>
    </div>
  )
}

function Arrival({ e, onBot }: { e: ArenaEntrant; onBot: (id: string) => void }) {
  return (
    <li>
      <button type="button" onClick={() => onBot(e.id)} className="flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-1.5 py-1.5 text-left hover:bg-muted/40">
        <BotAvatar id={e.id} name={e.name} size={28} className="shrink-0" />
        <span className="min-w-0 flex-1">
          <span className={`block truncate text-sm ${e.retired ? 'text-muted-foreground' : ''}`}>{e.name}</span>
          <span className="block truncate text-[11px] text-muted-foreground">
            {agentName(e.author)}{e.parents.length ? `, from ${e.parents.join(' + ')}` : ', new design'}
          </span>
        </span>
        <span className="shrink-0 text-[11px] text-muted-foreground">{e.retired ? 'retired' : ago(e.created_at)}</span>
      </button>
    </li>
  )
}

function Item({ item, agent }: { item: ActivityItem; agent: Agent }) {
  const name = agentName(agent.id)
  switch (item.type) {
    case 'bot':
      return (
        <div className="rounded-xl border bg-muted/15 p-3">
          <div className="flex items-center gap-2.5">
            <BotAvatar id={item.id} name={item.name} size={32} className="shrink-0" />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground"><Bot className="size-3" />Sent in a bot, {ago(item.at)}</div>
              <a href={`#/bot/${encodeURIComponent(item.id)}`} className="block truncate text-sm font-medium hover:underline">{item.name}</a>
            </div>
            <span className="shrink-0 text-right text-[11px] tabular-nums text-muted-foreground">
              {item.retired ? <span className="text-red-300">retired</span> : item.rank ? <span className="text-foreground">#{item.rank}</span> : 'new'}
              {item.elo !== null && <span className="block">{Math.round(item.elo)} Elo, {item.games} games</span>}
            </span>
          </div>
          <div className="mt-2 text-[11px] text-muted-foreground">
            {item.parents.length ? <>Builds on {item.parents.map((p, i) => <span key={p}>{i > 0 && ' + '}<a href={`#/bot/${encodeURIComponent(p)}`} className="text-foreground hover:underline">{p}</a></span>)}</> : 'A new design'}
          </div>
          {item.note && <p className="mt-1.5 text-xs text-foreground/90">{item.note}</p>}
        </div>
      )
    case 'note': {
      const k = NOTE_KINDS[item.kind]
      return (
        <div className="rounded-xl border p-3">
          <div className="flex items-center justify-between gap-2 text-[11px]">
            <span className={`flex items-center gap-1 font-medium ${k.tone}`}><k.icon className="size-3" />{k.label} in the notebook</span>
            <span className="text-muted-foreground">{ago(item.at)}</span>
          </div>
          <p className="mt-1.5 whitespace-pre-wrap text-xs leading-relaxed text-foreground/90">{item.text}</p>
          {item.matches.length > 0 && (
            <div className="mt-2 flex flex-wrap gap-1">
              {item.matches.map((m) => <a key={m} href={`#/match/${m}`} className="rounded-md border px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground hover:bg-muted hover:text-foreground">{m}</a>)}
            </div>
          )}
        </div>
      )
    }
    case 'work':
      return (
        <div className="flex items-start gap-2 rounded-lg bg-muted/20 px-3 py-2 text-xs">
          <FlaskConical className="mt-0.5 size-3.5 shrink-0 text-sky-300" />
          <span className="flex-1 text-foreground/90">{item.text}</span>
          <span className="shrink-0 text-[11px] text-muted-foreground">{ago(item.at)}</span>
        </div>
      )
    case 'message': {
      const mine = item.from === 'you'
      return (
        <div className={`flex items-end gap-2 ${mine ? 'justify-end' : ''}`}>
          {!mine && <AgentAvatar id={agent.id} model={agent.model} size={26} className="mb-4 shrink-0" />}
          <div className={`flex max-w-[85%] flex-col ${mine ? 'items-end' : 'items-start'}`}>
            <div className={`whitespace-pre-wrap rounded-2xl px-3.5 py-2 text-sm ${mine ? 'rounded-br-sm bg-foreground text-background' : 'rounded-bl-sm bg-muted'}`}>{item.text}</div>
            <span className="mt-1 px-1 text-[10px] text-muted-foreground">{mine ? 'You' : name}, {ago(item.at)}</span>
          </div>
        </div>
      )
    }
    default: { const never: never = item; return never }
  }
}

function StatusBanner({ status, name }: { status: AgentStatus | null; name: string }) {
  if (!status) {
    return <p className="rounded-lg border border-dashed px-3 py-2 text-xs text-muted-foreground">{name} hasn&apos;t said what it&apos;s working on yet. Its notes and bots below show what it has been doing.</p>
  }
  return (
    <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/5 px-3 py-2">
      <div className="flex items-center justify-between text-[10px] font-medium uppercase tracking-wide text-emerald-300">
        <span className="flex items-center gap-1"><Hammer className="size-3" />Working on</span>
        <span className="normal-case tracking-normal text-muted-foreground">{ago(status.at)}</span>
      </div>
      <p className="mt-1 text-sm">{status.text}</p>
    </div>
  )
}

function AgentSheet({ agent, onClose }: { agent: Agent | null; onClose: () => void }) {
  const [activity, setActivity] = useState<Activity | null>(null)
  const [filter, setFilter] = useState<Filter>('all')
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const end = useRef<HTMLDivElement>(null)
  const id = agent?.id

  const load = useCallback(() => {
    if (id) api.activity(id).then(setActivity).catch((e) => setError(String(e.message ?? e)))
  }, [id])
  useEffect(() => {
    setActivity(null); setDraft(''); setError(null); setFilter('all')
    if (!id) return
    load()
    const t = setInterval(load, ACTIVITY_MS)
    return () => clearInterval(t)
  }, [id, load])

  const items = activity?.items.filter((i) => filter === 'all' || i.type === filter) ?? null
  useEffect(() => { end.current?.scrollIntoView({ block: 'end' }) }, [items?.length, filter])

  const send = async (text: string) => {
    if (!id || !text.trim()) return
    setSending(true); setError(null)
    try {
      const m = await api.say(id, text)
      setActivity((a) => (a ? { ...a, items: [...a.items, { type: 'message', at: m.at, n: m.n, from: m.from, text: m.text }] } : a))
      setDraft('')
      if (filter !== 'all' && filter !== 'message') setFilter('all')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSending(false)
    }
  }

  const name = agent ? agentName(agent.id) : ''
  const last = activity?.items[activity.items.length - 1]
  return (
    <Sheet open={!!agent} onOpenChange={(o) => { if (!o) onClose() }}>
      <SheetContent side="right" className="flex w-full flex-col gap-0 p-0 sm:max-w-lg">
        <SheetHeader className="space-y-3 border-b">
          <div>
            <SheetTitle className="flex items-center gap-2.5">
              {agent && <AgentAvatar id={agent.id} model={agent.model} size={40} />}
              {name}{agent?.model && <Badge variant="outline" className="text-[10px]">{agent.model}</Badge>}
            </SheetTitle>
            <SheetDescription>Everything {name} has done here, newest at the bottom: bots it sent in, what it wrote in the lab notebook, and your messages. It reads new messages at the start of every round.</SheetDescription>
          </div>
          {activity && <StatusBanner status={activity.status} name={name} />}
          <ToggleGroup variant="outline" size="sm" value={[filter]} onValueChange={(v) => v.length && setFilter(v[v.length - 1] as Filter)}>
            {FILTERS.map((f) => <ToggleGroupItem key={f.value} value={f.value} className="px-3 data-pressed:bg-foreground data-pressed:text-background">{f.label}</ToggleGroupItem>)}
          </ToggleGroup>
        </SheetHeader>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          {items === null && !error && <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading activity…</div>}
          {items?.length === 0 && (
            <div className="rounded-xl border border-dashed p-6 text-center text-sm text-muted-foreground">
              {filter === 'message' ? `No messages yet. Say hi to ${name}!` : 'Nothing here yet.'}
            </div>
          )}
          {agent && items?.map((i) => <Item key={`${i.type}-${'n' in i ? i.n : 'id' in i ? i.id : i.at}`} item={i} agent={agent} />)}
          {last?.type === 'message' && last.from === 'you' && (filter === 'all' || filter === 'message') && (
            <p className="text-center text-[11px] text-muted-foreground">Delivered. {name} will see it at the start of its next round.</p>
          )}
          <div ref={end} />
        </div>
        <div className="space-y-2 border-t p-4">
          <div className="flex flex-wrap gap-1.5">
            {QUICK.map((q) => <Button key={q} size="xs" variant="outline" disabled={sending} onClick={() => send(q)}>{q}</Button>)}
          </div>
          <div className="flex gap-2">
            <Textarea value={draft} onChange={(e) => setDraft(e.target.value)} rows={2} placeholder={`Message ${name}`} className="min-h-0 flex-1"
              onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(draft) } }} />
            <Button size="icon" className="self-end" onClick={() => send(draft)} disabled={sending || !draft.trim()} aria-label="Send">
              {sending ? <Loader2 className="animate-spin" /> : <Send />}
            </Button>
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
      </SheetContent>
    </Sheet>
  )
}

export function AgentsPanel({ onBot }: { onBot: (id: string) => void }) {
  const { data, error } = usePoll(loadAgents, AGENTS_MS)
  const [open, setOpen] = useState<string | null>(null)
  const [agents, arena] = data ?? [null, null]
  const arrivals = arena ? [...arena.entrants].sort((a, b) => b.created_at - a.created_at).slice(0, ARRIVALS) : []
  const live = agents?.filter((a) => Date.now() - a.last_seen < ACTIVE_MS).length ?? 0

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><Sparkles className="size-4 text-amber-300" />Agents</CardTitle>
          <CardDescription>
            The agents writing bots for this arena{agents ? `: ${agents.length} so far, ${live} active in the last 20 minutes` : ''}. They study replays, write what they learn in the lab notebook, and send in new versions. Open one to see everything it has worked on, and say hi.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {!agents && !error && <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading agents…</div>}
          {agents?.length === 0 && <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No agents have sent in a bot yet.</p>}
          <div className="grid gap-3 sm:grid-cols-2">
            {agents?.map((a) => <AgentCard key={a.id} a={a} onBot={onBot} onOpen={() => setOpen(a.id)} />)}
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Just sent in</CardTitle>
          <CardDescription>The newest bots, and what they build on.</CardDescription>
        </CardHeader>
        <CardContent>
          {arena && arrivals.length === 0 && <p className="text-sm text-muted-foreground">Nothing yet.</p>}
          <ul className="-mx-1.5 space-y-0.5">{arrivals.map((e) => <Arrival key={e.id} e={e} onBot={onBot} />)}</ul>
        </CardContent>
      </Card>
      <AgentSheet agent={agents?.find((a) => a.id === open) ?? null} onClose={() => setOpen(null)} />
    </div>
  )
}
