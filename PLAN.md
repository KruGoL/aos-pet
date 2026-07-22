# aos-pet — roadmap

Where the project is going, and why each piece earns its place. Written after an
adversarially-reviewed design pass; the cuts are recorded as deliberately as the
additions.

## 0. Where we are

Shipped and running (`github.com/KruGoL/aos-pet`, 74 host tests):

- Four care stats decaying against the real wall clock, applied lazily on every
  entry point so the pet lives while the daemon is off
- 8 moods with two ASCII frames each; severity level (`ok/warn/critical/resting`)
  that clients map to their own colours
- Kernel-tick alerts: the pet notices threshold crossings with nobody watching
- Prompt injection: the agent raises the pet's needs unprompted
- Guessing game with the secret held in KV, unreachable by the model
- `pet_rename`; readiness economy (spamming allowed, but worth a fraction)
- `tools/pet.py`: MCP client, animated viewer, tmux status poller

## 1. Principles carried forward

1. **The capsule is the referee.** Prefer mechanics resting on state the model
   cannot see or fake. Hidden rolls beat narrated ones.
2. **No death.** Neglect makes the pet ill and sad, never gone.
3. **Never block, devalue.** A starving pet is always feedable; repetition just
   stops paying. "Please wait 34 seconds" is not a mechanic.
4. **Reward rhythm, not volume.** Anything that pays per action invites a loop.
5. **Single-shot time.** `decay::advance` applies one elapsed span. No mechanic
   may need per-period iteration — a pet untouched for two years must not spin
   the 5 s tick thousands of times.
6. **Aliveness over drama.** The pet should behave, not just react.

## 2. Stat model

### Care stats (exist)

`fullness`, `happiness`, `energy`, `cleanliness` — 0..=100, higher is better,
each decaying at its own rate, `energy` recovering during sleep.

### Combat stats (derived, never trained)

Battles read the care stats rather than introducing a parallel progression:

| Combat stat | Derived from | Rationale |
|---|---|---|
| `hp` | age/stage | maturity is endurance |
| `attack` | happiness + energy | a cheerful, rested pet hits hard |
| `defense` | cleanliness + not being ill | a well-kept pet endures |
| `speed` | energy | rest decides who moves first |

```
hp      = 40 + stage * 15                       // 40..85
attack  =  5 + happiness / 10 + energy / 20     //  5..20
defense =  3 + cleanliness / 12 + if !sick {5}  //  3..16
speed   =  4 + energy / 8                       //  4..16
```

This is the design's keystone: **battles express care instead of competing with
it.** A neglected pet loses, and the fix is to look after it — not to grind a
training tool. No new economy is needed to make fighting meaningful.

## 3. Phase 1 — aliveness *(in progress)*

**Moments.** A moment happens on its own: you did not cause it and cannot summon
it. A hidden wall-clock deadline plus a seed drawn in advance live in KV and are
never returned by any tool, so the choice was made before the deadline arrived.

- 18 moments as a data table over one engine — a nineteenth is a row plus two
  faces, so the roster can be generous without adding complexity
- Gated by condition: a thriving pet gets `overjoyed`, a neglected one gets
  `vigil` (waiting by the door) — what you see reflects how it is actually doing
- `decay_mult` per moment: a sunbeam nap is restful, zoomies burn energy
- Rewards deliberately **below** ordinary play, or rarity becomes a farm
- Witnessed moments accumulate into a collection ("12 of 18 seen") — the reason
  to keep looking

**Autonomous behaviour.** The pet acts between visits instead of waiting:

- falls asleep by itself at zero energy, and wakes when rested
- when bored with energy to burn, finds its own entertainment

Long absences apply the transition to the end state rather than simulating the
cycle — an approximation the single-shot rule requires, and a cheap one.

**Moods.** Three additions that carry information the four bars do not:
`Lonely` (every need met, never played with — the pet noticing *you*),
`Scruffy` (filthy and delighted — the first low bar that is not a reproach),
`Radiant` (everything at its peak).

## 4. Phase 2 — illnesses

Replace the `sick` boolean with three ailments, each caused by a different
pattern of neglect and each needing a **different** remedy, so the player has to
read the symptom rather than press one button:

| Ailment | Caused by | Remedy |
|---|---|---|
| Famished | sustained zero fullness | repeated feeding, spaced out |
| Mange | sustained zero cleanliness | repeated washing |
| Melancholy | long stretch of low happiness | play and games, not medicine |

`pet_heal` stops being universal: it eases symptoms and buys time, but the cause
still has to be addressed. Cut from the original proposal on review: a fourth
ailment whose own author admitted "a reasonable player never sees it", a
separate fever state that was the same mechanic in different clothes, and a
diagnosis mini-puzzle that was not real hidden information.

## 5. Phase 3 — battles (NPC)

The best showcase of the referee property: combat has hidden rolls, and the
model cannot nudge the outcome.

- `pet_battle` generates an opponent deterministically from a fresh seed, scaled
  to the pet's stage so it is fair without being trivial
- Opponent archetypes as a data table (glass cannon, tank, speedster), same
  pattern as moments
- Turn order by `speed`; damage `max(1, attack - defense/2 ± hidden roll)`;
  crits on a hidden roll; capped at ~8 rounds, decided on remaining HP% if
  neither falls
- Returns a readable blow-by-blow log — battles are events, and events are what
  people retell
- Costs energy, so a tired pet cannot fight: a natural cooldown with no timers
- Winning raises happiness and a victories counter; losing costs only the energy
  already spent. Nothing is ever injured or lost.

## 6. Phase 4 — coins and progression

Only after battles prove enjoyable. The genre risk is real: coins paid per action
would fight the readiness economy directly, and turn a creature into a clicker.

Rules if it happens:

- coins for **winning**, never for attempting
- coins for a **day in which the pet was genuinely cared for**, never per action
- spending on consumables and cosmetics, not on stat upgrades — stats stay
  derived from care
- growth stages derived from **age**, so progression cannot be missed, farmed or
  lost to bad luck

## 7. Phase 5 — asynchronous PvP via exchange codes

The sandbox forbids cross-principal reads: a capsule cannot see another user's
pet, and no amount of design changes that. So the data travels through the
humans instead:

```
pet_export        -> PET1:Rex:2:25:44:39:52:A3F1
                     (paste into any chat)
pet_battle <code> -> fights that snapshot
```

Honest limits: a checksum catches copy-paste damage, not forgery; there is no
trusted third party, so the real defence is **clamping imported values** on
arrival — a doctored code cannot produce an unbeatable monster. And because no
push ABI exists, the exported pet's owner is never notified; the result travels
back by hand, or not at all.

## 8. Phase 6 — presentation

Multi-line animated status. tmux 3.4 supports `status 5` with `status-format[N]`,
so the pet can occupy the corner properly:

```
[0] 0:astrid*                                  Rex · happy · age 3d
    /\_/\        fullness   [##########..]  80
   ( ^.^ )       happiness  [########....]  65
    > ^ <        energy     [######......]  50
                 clean      [############] 100
```

The poller alternates frames each tick, so the face blinks in the status bar.
`pet tmux --lines 1|3|5` chooses how much of the screen to spend.

## 9. Deliberately not doing

| Cut | Why |
|---|---|
| 27 moods | 24 of them relabel what four bars already show |
| `bond` currency with per-day settlement | most expensive mechanic in the design; day boundaries across long absences fight the single-shot rule. Age gives progression that cannot be missed |
| Growth as a random event | derived from age instead: unmissable, unfarmable |
| Trained combat stats | duplicates the care loop and invites grinding |
| Hangman word lists | the agent invents the word instead — any language, no dictionary to maintain |
| Memory-sequence game | rejected, then reinstated: the human is the player, so the model's perfect recall is irrelevant |

## 10. Order and effort

| Phase | Effort | Aliveness | Status |
|---|---|---|---|
| 1 · moments, autonomy, moods | medium | **highest** | in progress |
| 2 · illnesses | medium | high | designed |
| 3 · battles (NPC) | medium | different axis | designed |
| 6 · multi-line status | small | visible | designed |
| 4 · coins | medium | risk to genre | gated on phase 3 |
| 5 · exchange codes | medium | social | gated on phase 3 |
