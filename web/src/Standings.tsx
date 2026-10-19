import { ArrowDownRight, ArrowUpRight, Minus, Trophy } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { Leaderboard, LeaderboardRow } from './api'
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

function speed(ms: number) {
  return ms.toFixed(ms < 1 ? 3 : 2)
}

function Row({ r, start, retired, need, onBot }: { r: LeaderboardRow; start: number; retired?: string; need: number; onBot: (id: string) => void }) {
  return (
    <TableRow className="hover:bg-muted/30">
      <TableCell className="w-10 text-center font-semibold tabular-nums">
        {r.rank <= 3 ? <Trophy className={`mx-auto size-4 ${medal[r.rank - 1]}`} /> : r.rank}
      </TableCell>
      <TableCell className="font-medium">
        <button type="button" onClick={() => onBot(r.bot)} className={`cursor-pointer text-left hover:underline hover:underline-offset-4 ${retired ? 'text-muted-foreground' : ''}`} title="Open profile">
          <span className="inline-flex items-center gap-2"><BotAvatar id={r.bot} name={r.name} size={24} className="shrink-0" />{r.name}</span>
        </button>
        {retired && <span className="ml-1.5 text-[10px] uppercase tracking-wide text-red-300" title={retired}>retired</span>}
        {!r.rated && <span className="ml-1.5 text-[10px] uppercase tracking-wide text-amber-300" title={`Rating still settling: ${r.games} of ${need} games`}>provisional</span>}
      </TableCell>
      <TableCell className="text-right">
        <Tooltip>
          <TooltipTrigger render={<span className={`cursor-default text-base font-semibold tabular-nums ${r.rated ? '' : 'text-muted-foreground'}`} />}>{Math.round(r.elo)}</TooltipTrigger>
          <TooltipContent>Peak {Math.round(r.peak_elo)}</TooltipContent>
        </Tooltip>
      </TableCell>
      <TableCell className="w-14"><Change d={r.elo_change} /></TableCell>
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

export function Standings({ board, onBot }: { board: Leaderboard | null; onBot: (id: string) => void }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><Trophy className="size-4 text-amber-300" />Standings</CardTitle>
        <CardDescription>
          Elo rating for each CPU style across {board?.games ?? 0} finished game{board?.games === 1 ? '' : 's'} under rules {board?.rules_version ?? 'v0.6'}. Everyone starts at {board?.start_elo ?? 1500}.
          Each free-for-all counts as a head-to-head result between every pair of players, where the better place wins, and ratings update after every game (K = {board?.k ?? 32}). Win % and top half % are per seat; top half means finishing in the top half of the lobby (1st–2nd of 4, 1st–3rd of 6).
          A rating is <span className="text-amber-300">provisional</span> until the bot has played {board?.rated_games ?? 50} games: one 8-player result swings by about two places, so it takes about 50 to pin the average place down to ±0.3. The arena schedules the bots with the fewest games first, and a bot's first 20 games move its Elo three times as far.
          Any rated bot outside the top {board?.top ?? 20} is retired from the self-playing arena and the league's sparring seats (the league champion excepted) and leaves this table{board?.retired_bots?.length ? ` (${board.retired_bots.length} retired so far)` : ''}. Retired bots stay playable from the lineup picker.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {!board || board.bots.length === 0 ? (
          <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No finished games yet. Create a few below and the table fills in.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="text-center">#</TableHead>
                <TableHead>CPU</TableHead>
                <TableHead className="text-right">Elo</TableHead>
                <TableHead>Last</TableHead>
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
              {board.bots.map((r) => <Row key={r.bot} r={r} start={board.start_elo} retired={board.retired?.[r.bot]} need={board.rated_games ?? 50} onBot={onBot} />)}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}
