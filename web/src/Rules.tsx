import { BookText, Loader2 } from 'lucide-react'
import type { ReactNode } from 'react'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { api } from './api'
import { BuildingIcon } from './Legend'
import { Offline, Page } from './Page'
import { usePoll } from './poll'

const RULES_MS = 60_000
const loadRules = () => api.rules()

const pct = (milli: number) => `${Math.round(milli / 10)}%`
const times = (milli: number) => `×${(milli / 1000).toFixed(milli % 100 ? 2 : 1)}`

function Section({ title, description, children }: { title: string; description?: ReactNode; children: ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        {description && <CardDescription>{description}</CardDescription>}
      </CardHeader>
      <CardContent className="space-y-3 text-sm leading-relaxed text-muted-foreground [&_strong]:font-medium [&_strong]:text-foreground">{children}</CardContent>
    </Card>
  )
}

function Building({ kind, name, cost, where, children }: { kind: number; name: string; cost: number; where: string; children: ReactNode }) {
  return (
    <div className="flex gap-3 rounded-xl border bg-muted/20 p-3">
      <BuildingIcon kind={kind} size={34} />
      <div className="min-w-0 space-y-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium text-foreground">{name}</span>
          <Badge variant="outline" className="tabular-nums">{cost} troops</Badge>
          <span className="text-xs">{where}</span>
        </div>
        <div className="text-sm">{children}</div>
      </div>
    </div>
  )
}

export function RulesPage() {
  const { data: r, error, refresh } = usePoll(loadRules, RULES_MS)
  if (!r) {
    return (
      <Page icon={BookText} title="Rules" description="How a game of Hexarena plays, with the live numbers.">
        {error ? <Offline error={error} onRetry={refresh} /> : <div className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />Loading rules…</div>}
      </Page>
    )
  }
  const lobbies = Array.from({ length: r.max_players - r.min_players + 1 }, (_, i) => r.min_players + i)
  const radius = (n: number) => Math.round(r.map_radius_factor * n ** r.map_radius_exponent)
  const hexes = (rad: number) => 3 * rad * (rad + 1) + 1
  const discounted = (cost: number) => Math.ceil((cost * (100 - r.quarry.troop_discount_pct)) / 100)
  const overtimeFull = (r.overtime_start ?? 0) + Math.ceil((((r.overtime_max_win_milli ?? 0) - r.attacker_exchange_win_milli) * 1000) / Math.max(1, r.overtime_win_milli_per_1000 ?? 1))
  const walls = r.wall.defense_milli_by_links
  return (
    <Page icon={BookText} title="Rules" description={<>How a game of Hexarena plays. Every number here is read live from the server, so it always matches the games being played. <Badge variant="outline" className="ml-1 align-middle">rules {r.version}</Badge></>}>
      <Section title="Goal" description={`A free-for-all for ${r.min_players} to ${r.max_players} CPU players on a hex board, ${r.tick_rate_hz} ticks a second.`}>
        <p>Grow your territory, build an economy and take your rivals&apos; land. A player with no land left is <strong>eliminated</strong>, and the game ends the moment <strong>one player is left standing</strong>.</p>
        <p>If nobody has finished it by tick <strong>{r.max_ticks.toLocaleString()}</strong>, the game is cut off. Survivors are ranked by <strong>land</strong> (troops and then buildings only break exact ties), and below them the eliminated players, latest elimination first.</p>
        <p>For Elo, a game ended before the cutoff counts <strong>N times</strong> as much as a cutoff game in an N-player lobby, so closing out an 8-player game is worth 8 games that ran to the cutoff.</p>
      </Section>

      <Section title="The board" description="Lobbies of every size get a compact board, so big games still end.">
        <p>The board is a hexagon of radius <strong>{r.map_radius_factor} × players<sup>{r.map_radius_exponent}</sup></strong>, rounded. Mountains (impassable) cover {r.impassable_min_pct}–{r.impassable_max_pct}% of it, and the rest has heights from valleys to hills. Neutral hexes start with {r.neutral_min}–{r.neutral_max} troops. Everyone spawns on one hex with {r.starting_troops} troops, at least {r.spawn_min_distance} hexes from anyone else.</p>
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Players</TableHead>
              {lobbies.map((n) => <TableHead key={n} className="text-right tabular-nums">{n}</TableHead>)}
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow className="hover:bg-transparent">
              <TableCell>Radius</TableCell>
              {lobbies.map((n) => <TableCell key={n} className="text-right tabular-nums">{radius(n)}</TableCell>)}
            </TableRow>
            <TableRow className="hover:bg-transparent">
              <TableCell>Hexes per player</TableCell>
              {lobbies.map((n) => <TableCell key={n} className="text-right tabular-nums">{Math.round(hexes(radius(n)) / n)}</TableCell>)}
            </TableRow>
          </TableBody>
        </Table>
        <p>Terrain is public. Ownership, troops and buildings are hidden by fog: you see {r.fog_radius} hexes beyond your own land.</p>
      </Section>

      <Section title="Each tick" description="One action per player per tick.">
        <ul className="list-disc space-y-1 pl-5">
          <li><strong>Move</strong> troops from one of your hexes to an adjacent hex: into your own land to reinforce, or into neutral or enemy land to attack.</li>
          <li><strong>Gather</strong>: every hex with spare troops steps them one hex toward a target through your land.</li>
          <li><strong>Build</strong> on one of your hexes, paying in troops from it.</li>
          <li><strong>Pass</strong>.</li>
        </ul>
        <p>Every hex you own makes <strong>{r.base_income} troop</strong> per tick, on that hex. There is no other currency: troops pay for everything.</p>
      </Section>

      <Section title="Combat" description="Attacks are fought one exchange at a time.">
        <p>In each exchange the attacker wins with a <strong>{pct(r.attacker_exchange_win_milli)}</strong> chance. A win takes away one defender, divided by the hex&apos;s defense; a loss costs one attacker. The attack takes the hex once the defenders are gone, and the survivors move in. A captured building changes hands with the hex.</p>
        <p><strong>Defense</strong> is {times(r.height_factor_milli)} per level the defender stands above the attacker (and ÷{(r.height_factor_milli / 1000).toFixed(1)} per level below), times any Wall, capped at {times(r.maximum_total_defense_milli)}.</p>
        {(r.overtime_win_milli_per_1000 ?? 0) > 0 && (
          <p><strong>Overtime:</strong> from tick {(r.overtime_start ?? 0).toLocaleString()} the attacker&apos;s chance rises {pct(r.overtime_win_milli_per_1000 ?? 0)} every 1,000 ticks, up to {pct(r.overtime_max_win_milli ?? 0)} at about tick {overtimeFull.toLocaleString()}, so held borders eventually break.</p>
        )}
        <p><strong>Encirclement:</strong> a piece of enemy land surrounded entirely by yours, with no board edge or mountain in the ring, surrenders to you with its troops.</p>
      </Section>

      <Section title="Buildings" description={`A Quarry discounts buildings within ${r.quarry.discount_radius} hex by ${r.quarry.troop_discount_pct}%.`}>
        <div className="grid gap-3">
          <Building kind={2} name="Village" cost={r.village.troop_cost} where={`any height · ${discounted(r.village.troop_cost)} next to your Quarry`}>
            +{r.village.bonus} troop per tick on its hex, plus {r.village.farm_bonus} for every adjacent Farm{(r.quarry.village_bonus ?? 0) > 0 && ` and ${r.quarry.village_bonus} for every adjacent Quarry`}. A Village ringed by Farms is the core of most economies.
          </Building>
          <Building kind={1} name="Farm" cost={r.farm.troop_cost} where={`low ground (heights ${r.farm.allowed_heights.join(', ')}) · ${discounted(r.farm.troop_cost)} next to your Quarry`}>
            +{r.village.farm_bonus} troop per tick to every Village next to it.
          </Building>
          <Building kind={3} name="Quarry" cost={r.quarry.troop_cost} where={`hills (heights ${r.quarry.allowed_heights.join(', ')}), ${r.quarry.min_friendly_distance}+ hexes from your other Quarries`}>
            Every building on an adjacent hex costs {r.quarry.troop_discount_pct}% less{(r.quarry.village_bonus ?? 0) > 0 && `, and each Village next to it makes +${r.quarry.village_bonus} troop per tick`}.
          </Building>
          <Building kind={4} name="Wall" cost={r.wall.troop_cost} where={`any height · ${discounted(r.wall.troop_cost)} next to your Quarry`}>
            Multiplies the hex&apos;s defense by {walls.map(times).join(', ')} with {walls.map((_, i) => i).join(', ')} link{walls.length > 2 ? 's' : ''} to neighbouring Walls (up to {r.wall.max_links}), so lines of Walls hold best.
          </Building>
        </div>
      </Section>
    </Page>
  )
}
