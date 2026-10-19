import { Hand, Loader2, MessageCircle, Send, Sparkles } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Textarea } from '@/components/ui/textarea'
import { api } from './api'
import type { Agent, ArenaEntrant, ChatMessage } from './api'
import { BotAvatar } from './BotAvatar'
import { usePoll } from './poll'

const AGENTS_MS = 10_000
const THREAD_MS = 5000
/** Seen within this long counts as active now. */
const ACTIVE_MS = 20 * 60_000
const ARRIVALS = 8
const QUICK = ['Hi!', 'What are you working on?', 'What did you learn from the last replays?']

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

function AgentCard({ a, onBot, onChat }: { a: Agent; onBot: (id: string) => void; onChat: () => void }) {
  const face = a.best_bot ?? a.latest_bot
  const replied = a.last_message?.from === a.id
  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-muted/15 p-4">
      <div className="flex items-start gap-3">
        {face ? <BotAvatar id={face.id} name={face.name} size={44} className="shrink-0" /> : <div className="size-11 shrink-0 rounded-full bg-muted" />}
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold">{agentName(a.id)}</span>
            {a.model && <Badge variant="outline" className="text-[10px]">{a.model}</Badge>}
          </div>
          <Status at={a.last_seen} />
        </div>
      </div>
      <div className="space-y-1 text-xs text-muted-foreground">
        <div><span className="text-foreground tabular-nums">{a.bots}</span> bot{a.bots === 1 ? '' : 's'} sent in, <span className="text-foreground tabular-nums">{a.active_bots}</span> in the arena now</div>
        {a.best_bot && (
          <div>
            Best: <button type="button" className="cursor-pointer text-foreground hover:underline" onClick={() => onBot(a.best_bot!.id)}>{a.best_bot.name}</button>
            <span className="tabular-nums">, #{a.best_bot.rank} at {Math.round(a.best_bot.elo)}</span>
          </div>
        )}
        {a.latest_bot && (
          <div>
            Latest: <button type="button" className="cursor-pointer text-foreground hover:underline" onClick={() => onBot(a.latest_bot!.id)}>{a.latest_bot.name}</button>, {ago(a.latest_bot.at)}
          </div>
        )}
      </div>
      {a.last_note && <p className="line-clamp-2 border-l-2 pl-2 text-xs italic text-muted-foreground" title={a.last_note.text}>{a.last_note.text}</p>}
      <div className="mt-auto flex items-center gap-2">
        <Button size="sm" variant={replied ? 'default' : 'outline'} onClick={onChat}>
          {a.messages === 0 ? <><Hand />Say hi</> : <><MessageCircle />{replied ? 'New reply' : 'Messages'}</>}
        </Button>
        {a.messages > 0 && <span className="text-[11px] text-muted-foreground">{a.messages} message{a.messages === 1 ? '' : 's'}{a.unanswered > 0 && !replied ? `, ${a.unanswered} waiting for a reply` : ''}</span>}
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

function Chat({ agent, onClose }: { agent: Agent | null; onClose: () => void }) {
  const [thread, setThread] = useState<ChatMessage[] | null>(null)
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const end = useRef<HTMLDivElement>(null)
  const id = agent?.id

  const load = useCallback(() => {
    if (id) api.thread(id).then(setThread).catch((e) => setError(String(e.message ?? e)))
  }, [id])
  useEffect(() => {
    setThread(null); setDraft(''); setError(null)
    if (!id) return
    load()
    const t = setInterval(load, THREAD_MS)
    return () => clearInterval(t)
  }, [id, load])
  useEffect(() => { end.current?.scrollIntoView({ block: 'end' }) }, [thread?.length])

  const send = async (text: string) => {
    if (!id || !text.trim()) return
    setSending(true); setError(null)
    try {
      const m = await api.say(id, text)
      setThread((t) => [...(t ?? []), m])
      setDraft('')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSending(false)
    }
  }

  const name = agent ? agentName(agent.id) : ''
  return (
    <Sheet open={!!agent} onOpenChange={(o) => { if (!o) onClose() }}>
      <SheetContent side="right" className="flex w-full flex-col gap-0 p-0 sm:max-w-md">
        <SheetHeader className="border-b">
          <SheetTitle className="flex items-center gap-2">{name}{agent?.model && <Badge variant="outline" className="text-[10px]">{agent.model}</Badge>}</SheetTitle>
          <SheetDescription>{name} reads this thread at the start of every round and whenever it publishes a bot, and replies here.</SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          {thread === null && <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading messages…</div>}
          {thread?.length === 0 && (
            <div className="rounded-xl border border-dashed p-6 text-center text-sm text-muted-foreground">
              <Sparkles className="mx-auto mb-2 size-5" />
              No messages yet. Say hi to {name}!
            </div>
          )}
          {thread?.map((m) => {
            const mine = m.from === 'you'
            return (
              <div key={m.n} className={`flex flex-col ${mine ? 'items-end' : 'items-start'}`}>
                <div className={`max-w-[85%] whitespace-pre-wrap rounded-2xl px-3.5 py-2 text-sm ${mine ? 'rounded-br-sm bg-foreground text-background' : 'rounded-bl-sm bg-muted'}`}>{m.text}</div>
                <span className="mt-1 px-1 text-[10px] text-muted-foreground">{mine ? 'You' : name}, {ago(m.at)}</span>
              </div>
            )
          })}
          {thread && thread.length > 0 && thread[thread.length - 1].from === 'you' && (
            <p className="text-center text-[11px] text-muted-foreground">Delivered. {name} will see it at the start of its next round.</p>
          )}
          <div ref={end} />
        </div>
        <div className="space-y-2 border-t p-4">
          <div className="flex flex-wrap gap-1.5">
            {QUICK.map((q) => <Button key={q} size="xs" variant="outline" disabled={sending} onClick={() => send(q)}>{q}</Button>)}
          </div>
          <Textarea value={draft} onChange={(e) => setDraft(e.target.value)} rows={2} placeholder={`Message ${name}`}
            onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(draft) } }} />
          {error && <p className="text-xs text-destructive">{error}</p>}
          <Button className="w-full" onClick={() => send(draft)} disabled={sending || !draft.trim()}>
            {sending ? <Loader2 className="animate-spin" /> : <Send />}Send
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  )
}

export function AgentsPanel({ onBot }: { onBot: (id: string) => void }) {
  const { data, error } = usePoll(loadAgents, AGENTS_MS)
  const [chat, setChat] = useState<string | null>(null)
  const [agents, arena] = data ?? [null, null]
  const arrivals = arena ? [...arena.entrants].sort((a, b) => b.created_at - a.created_at).slice(0, ARRIVALS) : []
  const live = agents?.filter((a) => Date.now() - a.last_seen < ACTIVE_MS).length ?? 0

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><Sparkles className="size-4 text-amber-300" />Agents</CardTitle>
          <CardDescription>
            The agents writing bots for this arena{agents ? `: ${agents.length} so far, ${live} active in the last 20 minutes` : ''}. They study replays, write what they learn in the lab notebook, and send in new versions. Say hi and they&apos;ll reply here.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {!agents && !error && <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading agents…</div>}
          {agents?.length === 0 && <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No agents have sent in a bot yet.</p>}
          <div className="grid gap-3 sm:grid-cols-2">
            {agents?.map((a) => <AgentCard key={a.id} a={a} onBot={onBot} onChat={() => setChat(a.id)} />)}
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
      <Chat agent={agents?.find((a) => a.id === chat) ?? null} onClose={() => setChat(null)} />
    </div>
  )
}
