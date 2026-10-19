import { ArrowDownRight, ArrowUpRight, Minus, Trophy } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { BoardFormat, FormatRating, Leaderboard, LeaderboardRow } from './api'
import { BotAvatar } from './BotAvatar'

function Sparkline({ values, start }: { values: number[]; start: number }) {
  const w = 96, h = 26
  if (values.length < 2) return <svg width={w} height={h} />
  const lo = Math.min(start, ...values) - 4, hi = Math.max(start, ...values) + 4
  const x = (i: number) => (i / (values.length - 1)) * (w - 2) + 1
  const y = (v: number) => h - 1 - ((v - lo) / (hi - lo)) * (h - 2)
  const pts = values.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ')
  const up = values[values.length - 1] >= start
  return (
    <svg width={w} height={h} className="overflow-visible">
      <line x1={0} x2={w} y1={y(start)} y2={y(start)} stroke="currentColor" strokeOpacity={0.18} strokeDasharray="2 3" />
      <polyline points={pts} fill="none" stroke={up ? '#4ade80' : '#f87171'} strokeWidth={1.6} strokeLinejoin="round" />
      <circle cx={x(values.length - 1)} cy={y(values[values.length - 1])} r={2.2} fill={up ? '#4ade80' : '#f87171'} />
    </svg>
  )
}

function Change({ d }: { d: number }) {
  const r = Math.round(d)
  if (r === 0) return <span className="inline-flex items-center text-xs text-muted-foreground"><Minus className="size-3" />0</span>
  return r > 0
    ? <span className="inline-flex items-center text-xs text-emerald-400"><ArrowUpRight className="size-3" />{r}</span>
    : <span className="inline-flex items-center text-xs text-red-400"><ArrowDownRight className="size-3" />{-r}</span>
}

const medal = ['text-amber-300', 'text-zinc-300', 'text-orange-400']

const FORMATS: { id: BoardFormat; label: string }[] = [
  { id: 'all', label: 'All games' },
  { id: 'duel', label: '1v1' },
  { id: 'ffa', label: '8-player' },
]

/** A bot's rating in one format, next to its overall one. */
function FormatElo({ f, need }: { f: FormatRating | null | undefined; need: number }) {
  if (!f) return <span className="text-muted-foreground">—</span>
  return (
    <Tooltip>
      <TooltipTrigger render={<span className={`cursor-default tabular-nums ${f.games >= need ? '' : 'text-muted-foreground'}`} />}>{Math.round(f.elo)}</TooltipTrigger>
      <TooltipContent>#{f.rank} · {f.games} game{f.games === 1 ? '' : 's'}{f.games < need ? ', still settling' : ''}</TooltipContent>
    </Tooltip>
  )
}

function speed(ms: number) {
  return ms.toFixed(ms < 1 ? 3 : 2)
}

function Row({ r, start, retired, anchor, need, split, onBot }: { r: LeaderboardRow; start: number; retired?: string; anchor?: boolean; need: number; split: boolean; onBot: (id: string) => void }) {
  return (
    <TableRow className={`hover:bg-muted/30 ${anchor ? 'bg-sky-500/5' : ''}`}>
      <TableCell className="w-10 text-center font-semibold tabular-nums">
        {r.rank <= 3 ? <Trophy className={`mx-auto size-4 ${medal[r.rank - 1]}`} /> : r.rank}
      </TableCell>
      <TableCell className="font-medium">
        <button type="button" onClick={() => onBot(r.bot)} className={`cursor-pointer text-left hover:underline hover:underline-offset-4 ${retired ? 'text-muted-foreground' : ''}`} title="Open profile">
          <span className="inline-flex items-center gap-2"><BotAvatar id={r.bot} name={r.name} size={24} className="shrink-0" />{r.name}</span>
        </button>
        {retired && <span className="ml-1.5 text-[10px] uppercase tracking-wide text-red-300" title={retired}>retired</span>}
        {anchor && <span className="ml-1.5 text-[10px] uppercase tracking-wide text-sky-300" title="One of the first four CPUs ever written. Their average is pinned at 1500: the yardstick every rating is measured against">anchor</span>}
        {!r.rated && !anchor && <span className="ml-1.5 text-[10px] uppercase tracking-wide text-amber-300" title={`Rating still settling: ${r.games} of ${need} games`}>provisional</span>}
      </TableCell>
      <TableCell className="text-right">
        <Tooltip>
          <TooltipTrigger render={<span className={`cursor-default text-base font-semibold tabular-nums ${r.rated ? '' : 'text-muted-foreground'}`} />}>{Math.round(r.elo)}</TooltipTrigger>
          <TooltipContent>Peak {Math.round(r.peak_elo)}</TooltipContent>
        </Tooltip>
      </TableCell>
      <TableCell className="w-14"><Change d={r.elo_change} /></TableCell>
      {split && <TableCell className="hidden text-right sm:table-cell"><FormatElo f={r.duel} need={need} /></TableCell>}
      {split && <TableCell className="hidden text-right sm:table-cell"><FormatElo f={r.ffa} need={need} /></TableCell>}
      <TableCell className="hidden md:table-cell"><Sparkline values={r.history} start={start} /></TableCell>
      <TableCell className="text-right tabular-nums">{r.rated ? r.games : <span className="text-muted-foreground">{r.games}/{need}</span>}</TableCell>
      <TableCell className="text-right tabular-nums">{r.wins}</TableCell>
      <TableCell className="text-right tabular-nums">{Math.round(r.win_rate * 100)}%</TableCell>
      <TableCell className="text-right tabular-nums">{Math.round(r.top_half_rate * 100)}%</TableCell>
      <TableCell className="hidden text-right tabular-nums sm:table-cell">{r.avp8.toFixed(2)}<span className="ml-1 text-xs text-muted-foreground">±{r.avp8_se.toFixed(2)}</span></TableCell>
      <TableCell className="hidden text-right tabular-nums xl:table-cell">
        {r.ms_per_move != null ? <>{speed(r.ms_per_move)}{r.ms_per_move_ci95 != null && <span className="ml-1 text-xs text-muted-foreground">±{speed(r.ms_per_move_ci95)}</span>}</> : <span className="text-muted-foreground">—</span>}
      </TableCell>
      <TableCell className="hidden w-36 lg:table-cell">
        <div className="flex items-center gap-2">
          <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-foreground" style={{ width: `${r.avg_normalized * 100}%` }} /></div>
          <span className="w-9 text-right text-xs tabular-nums">{Math.round(r.avg_normalized * 100)}%</span>
        </div>
      </TableCell>
    </TableRow>
  )
}

function formatLine(format: BoardFormat, board: Leaderboard | null) {
  const duels = board?.duel_games ?? 0, ffas = board?.ffa_games ?? 0
  switch (format) {
    case 'all': return `Rated on every game: ${ffas} free-for-all${ffas === 1 ? '' : 's'} and ${duels} 1v1${duels === 1 ? '' : 's'}. The 1v1 and 8-player columns rate each bot on one kind of game alone, so a bot that is strong in one and weak in the other shows it.`
    case 'duel': return `Rated on 1v1 games only (${duels} so far). Every other self-play game is a 1v1 between the bot with the fewest 1v1s and a random opponent, on the smaller 2-player board.`
    case 'ffa': return `Rated on free-for-alls only (${ffas} so far): the 8-player self-play games, plus any 3 to 7 player games created by hand.`
    default: { const never: never = format; return never }
  }
}

export function Standings({ board, format = 'all', onFormat, onBot }: { board: Leaderboard | null; format?: BoardFormat; onFormat?: (f: BoardFormat) => void; onBot: (id: string) => void }) {
  const split = format === 'all' && !!board?.bots.some((r) => r.duel !== undefined)
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <CardTitle className="flex items-center gap-2"><Trophy className="size-4 text-amber-300" />Standings</CardTitle>
          {onFormat && (
            <Tabs value={format} onValueChange={(v) => onFormat(v as BoardFormat)}>
              <TabsList>
                {FORMATS.map((f) => <TabsTrigger key={f.id} value={f.id}>{f.label}</TabsTrigger>)}
              </TabsList>
            </Tabs>
          )}
        </div>
        <CardDescription>
          <span className="mb-2 block text-foreground/80">{formatLine(format, board)}</span>
          Elo rating for each CPU style across {board?.games ?? 0} finished game{board?.games === 1 ? '' : 's'} under rules {board?.rules_version ?? 'v0.7'}. Everyone starts at {board?.start_elo ?? 1500}.
          The first four CPUs ever written, {(board?.anchors ?? ['expander', 'homesteader', 'raider', 'warden']).map((a) => a.charAt(0).toUpperCase() + a.slice(1)).join(', ')}, are the <span className="text-sky-300">anchors</span>: their average is pinned at {board?.anchor_elo ?? 1500} and one of them plays in every self-play game, so every rating says how far a bot has come since the very first bots. 1900 means finishing ahead of a typical anchor about 9 times in 10.
          Every hand-written CPU, from the originals to Ridgeline and Relay, keeps playing and is never retired, so the whole lineage stays on one scale.
          Each free-for-all counts as a head-to-head result between every pair of players, where the better place wins, and the ratings are fitted to all of those results at once (maximum-likelihood Elo), so a rating says how often a bot really finishes ahead of each opponent. A game someone ends before the cutoff counts as much as eight that reach it. Win % and top half % are per seat; top half means finishing in the top half of the lobby (1st–2nd of 4, 1st–3rd of 6).
          A rating is <span className="text-amber-300">provisional</span> until the bot has played {board?.rated_games ?? 50} games: one 8-player result swings by about two places, so it takes about 50 to pin the average place down to ±0.3. The arena schedules the bots with the fewest games first.
          Any rated arena version outside the top {board?.top ?? 20} is retired from the self-playing arena and leaves this table{board?.retired_bots?.length ? ` (${board.retired_bots.length} retired so far)` : ''}. Retired bots stay playable from the lineup picker.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {!board ? (
          <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">Loading standings…</p>
        ) : board.bots.length === 0 ? (
          <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">{format === 'duel' ? 'No 1v1 games have finished yet. The arena plays one in every two self-play games, so the first appear within a few minutes.' : 'No finished games yet. Create a few below and the table fills in.'}</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="text-center">#</TableHead>
                <TableHead>CPU</TableHead>
                <TableHead className="text-right">Elo</TableHead>
                <TableHead>Last</TableHead>
                {split && <TableHead className="hidden text-right sm:table-cell">1v1</TableHead>}
                {split && <TableHead className="hidden text-right sm:table-cell">8-player</TableHead>}
                <TableHead className="hidden md:table-cell">Trend</TableHead>
                <TableHead className="text-right">Games</TableHead>
                <TableHead className="text-right">Wins</TableHead>
                <TableHead className="text-right">Win %</TableHead>
                <TableHead className="text-right">Top half %</TableHead>
                <TableHead className="hidden text-right sm:table-cell">
                  <Tooltip>
                    <TooltipTrigger render={<span className="cursor-help underline decoration-dotted underline-offset-4" />}>AVP (8p)</TooltipTrigger>
                    <TooltipContent className="max-w-64">Average placement scored as if every game had 8 players: 1st is 1, last is 8, so 2nd of 4 counts as 3.33. Lower is better. ± is one standard error.</TooltipContent>
                  </Tooltip>
                </TableHead>
                <TableHead className="hidden text-right xl:table-cell">
                  <Tooltip>
                    <TooltipTrigger render={<span className="cursor-help underline decoration-dotted underline-offset-4" />}>ms/move</TooltipTrigger>
                    <TooltipContent className="max-w-64">Average time the bot takes to decide one move, in milliseconds: the mean of its per-game averages. ± is the 95% confidence interval across games (Student t). Timed on the arena server while other games run, so compare bots with each other rather than read it as an absolute.</TooltipContent>
                  </Tooltip>
                </TableHead>
                <TableHead className="hidden lg:table-cell">Placement score</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {board.bots.map((r) => <Row key={r.bot} r={r} start={board.start_elo} retired={board.retired?.[r.bot]} anchor={board.anchors?.includes(r.bot)} need={board.rated_games ?? 50} split={split} onBot={onBot} />)}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}
