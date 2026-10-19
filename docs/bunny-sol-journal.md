# Bunny Lab (Sol) arena journal

Clean-room work for Hexarena rules v0.3. All strategies use only the fog-filtered `View`.

| Version | Hypothesis | Local / gate observation | Lesson |
|---|---|---|---|
| snowshoe-v1 — Frost Fences | Add one resource farm to Ridgeline, then put linked walls on hot borders. | 5.55 builds/game over 49 seats; gate remained queued. | The planner must deliberately Gather toward its first build; ordinary Ridgeline garrisons never naturally reach building cost. |
| snowshoe-v2 — Alpine Orchard | Compound one Farm into five linked Villages. | 4.95 builds/game over 21 seats. Gate rejected: +0.0208 ± 0.0843 over 96 games. | Full economic saturation was competitive but missed the +0.05 promotion floor; Villages beyond the Farm's three sockets are lower value. |
| snowshoe-v3 — Marten Homestead | Mix two Villages with five frontier Walls. | 7.43 builds/game over 49 seats. Gate rejected: +0.0382 ± 0.1058 over 96 games. | The best measured gate gain, but a large mixed cap churns captured walls and is noisy. |
| snowshoe-v4 — Beaver Quarryworks | Gather an early Quarry, then exploit its discount for a mixed cluster. | 5.18 builds/game over 49 seats; 0.78 Quarries/game. Gate remained queued. | Quarry construction works reliably, but its 150-troop opportunity cost is substantial. |
| snowshoe-v5 — Stoat Palisade | Cap the successful wall plan at three walls. | 4.25 builds/game over 49 seats. Gate rejected: -0.2708 ± 0.1541 over 16 games. | Arena rank was misleading; sparse walls lost the mirrored gate decisively. |
| snowshoe-v6 — Ibex Redoubt | Buy only two wall anchors and preserve a thick reserve. | 3.02 builds/game over 49 seats. Gate rejected vs g12: -0.5833 ± 0.2578 over 16 games. | Too little infrastructure did not preserve enough land tempo. |
| snowshoe-v7 — Foxglove Delay | Take land first, beginning construction at tick 900. | 3.74 builds/game over 49 seats. Gate rejected vs g12: -0.3125 ± 0.1875 over 16 games. | Delaying construction helped versus v6, but remained clearly negative. |
| snowshoe-v8 — Caribou Charge | Stop building and ignore safety for a 600-tick closing sprint. | 6.18 builds/game over 49 seats. Gate rejected vs g12: -0.7708 ± 0.1217 over 16 games. | A 600-tick liquidation sprint was the worst wall-family gate. |
| snowshoe-v9 — Wolverine Leanwall | Let wall defense replace one sixth of the champion's garrison margin. | 6.27 builds/game over 49 seats. Gate rejected vs g12: -0.3958 ± 0.3085 over 16 games. | Lowering the garrison margin did not convert wall defense into a reliable advantage. |
| snowshoe-v10 — Otter Millpond | Limit economy to one Farm and two Villages. | 2.71 builds/game over 48 seats. Gate rejected vs g12: -0.4792 ± 0.1619 over 16 games. | A sparse economy underperformed the larger v2 engine. |
| snowshoe-v11 — Badger Hearth | Test the minimum linked Farm–Village engine. | 1.67 builds/game over 48 seats. Gate rejected vs g12: -0.3958 ± 0.1835 over 16 games. | One pair does not compound enough to repay its action cost in the gate. |
| snowshoe-v12 — Jackalope Last Stand | Fund three Walls with one Village, then sprint. | 4.42 builds/game over 48 seats. Gate rejected vs g12: -0.7083 ± 0.1598 over 16 games. | Combining economy, walls and sprint accumulated costs instead of synergies. |
| snowshoe-v13 — Coyote Contact Line | Hard-cap lifetime replacement cost and wall only at visible enemy contact. | 4.73 builds/game over 26 seats; arena-retired. Gate rejected vs g12: -0.4792 ± 0.1525 over 16 games. | Refusing neutral-border walls did not improve results. |
| snowshoe-v14 — Pine Marten Dividend | Keep v2's economy to exactly the Farm's three supported Village sockets. | 3.65 builds/game over 48 seats. Gate rejected vs g12: -0.9167 ± 0.0891 over 16 games. | The three-socket cap was much worse against the promoted g12; v2's extra Villages were not merely waste. |

