# aos-pet

A virtual pet that lives inside a [Unicity AOS](https://github.com/unicity-aos/aos-ce) /
[Astrid](https://github.com/astrid-runtime/astrid) capsule.

```
    /\_/\
   ( ^.^ )
    > ^ <

  Rex — happy
  Fullness    [##########..]  80
  Happiness   [##########..]  80
  Energy      [##########..]  80
  Cleanliness [##########..]  80
  age 0h
```

Talk to your agent normally and it will bring the pet up on its own:

> **you:** привет
> **agent:** Hello! … By the way, your pet Rex is in bad shape — he's ill and all his
> stats have dropped to zero. Want me to heal him right away?

## Why this is more than a toy

**The capsule is the referee.** An LLM can happily hallucinate that your pet is fine.
This one can't: every stat lives in principal-scoped KV inside the sandbox, decays against
the real wall clock, and only changes through typed tool calls. The model can *ask* to
feed the pet; it cannot *decide* the pet is fed.

Three ideas it demonstrates concretely:

| Idea | How |
|---|---|
| **Lazy decay** | The capsule never ticks in the background. It stores `last_seen` and, on every entry point, applies however much real time has passed. The pet keeps living while the daemon is stopped or the machine sleeps — you can't pause it by shutting AOS down. |
| **Self-wake** | It subscribes to the kernel's `astrid.v1.watchdog.tick` (every 5s) so it notices it got hungry with nobody watching, and records an alert. |
| **Ambient awareness** | It injects a one-line briefing into the agent's system prompt, so the agent raises the pet's needs unprompted instead of waiting to be asked. |
| **No backend** | Built-in KV *is* the persistence layer. No database, no server, no migrations. KV is principal-scoped, so every user gets their own pet for free. |

## Tools

| Tool | Effect |
|---|---|
| `pet_adopt` | Adopt a pet and name it (`replace: true` to start over) |
| `pet_status` | Art, mood and every stat bar |
| `pet_feed` | Fullness ↑ |
| `pet_play` | Happiness ↑, energy ↓ — refuses if asleep or physically ill |
| `pet_sleep` | Sleep / wake (`wake: true`). Energy recovers while asleep |
| `pet_clean` | Cleanliness → 100 |
| `pet_heal` | Medicine — eases illness, but never a substitute for care |
| `pet_alerts` | What happened while you were away |
| `pet_rename` | Rename without losing age, stats or history |
| `pet_game_start` | Begin a guessing round |
| `pet_game_guess` | Make a guess |
| `pet_moments` | The collection — which rare moments you have witnessed |
| `pet_battle` | A friendly scrap with a passing stranger |

Neglect makes the pet **ill, never dead** — every illness is always curable.

### Three illnesses, three different cures

There is no single "sick" state and no one button that fixes it. Neglect a
particular need long enough and the pet earns the matching ailment:

| It is… | Because you | What actually fixes it |
|---|---|---|
| weak from hunger | let it starve | feed it, regularly |
| itchy and sore from the dirt | left it filthy | wash it, more than once |
| sunk in gloom | never played with it | play with it — **medicine will not lift this** |

`pet_heal` is real medicine, but it is not a master key: it takes the edge off
hunger and grime, cures neither outright, and does nothing at all for gloom. It
buys you time to do the actual thing.

Recovery obeys the same anti-grind rule as everything else — a remedy is worth
what it is worth *because you spaced it out*. Three impatient clicks in a row
will not undo a week of neglect, and that includes medicine: `pet_heal` has its
own clock, so hammering it is worth almost nothing.

A gloomy pet will always still play with you and still take you up on a game,
because those are its only way back. It will refuse a fight — a miserable pet
has no business scrapping — but it will tell you plainly that what it needs is
company, not a doctor.

### It lives while you are away

The pet is not an object waiting for input. On the kernel's 5-second tick it
looks after itself and occasionally just *does* something:

```
- Мурзик is getting hungry.
- Мурзик has found a patch of sun          <- a rare moment, unprompted
- Мурзик curled up and fell asleep on its own.
- Мурзик slept itself out and got up.
```

Eighteen **rare moments** run off a hidden wall-clock deadline and a seed drawn
in advance, both living in KV and never returned by any tool — so no amount of
polling can summon, repeat or steer one. Gates tie what happens to how the pet
is actually doing: a thriving pet gets `overjoyed`, a neglected one waits by the
door. A moment takes over the artwork while it lasts and is mentioned in the
agent's system prompt, so the agent brings it up without being asked.

`pet_moments` tracks which ones you have witnessed — and moments only count when
somebody looks. The watchdog can start one unattended; seeing it is on you.

### Battles

```
  a hedgehog rolls up and waits

    R1: Мурзик hits for 10 (critical!)
    R1: a hedgehog hits back for 7 (critical!)
    ...
    R5: Мурзик hits for 9 — and that settles it

  >> Мурзик sends a hedgehog packing!
     hp left: you 4 / them 0  ·  victories: 1
```

Fighting ability is **derived from care, never trained**:

```
hp      = 40 + stage * 15                       // stage comes from age alone
attack  =  5 + happiness / 10 + energy / 20
defense =  3 + cleanliness / 12 + if !sick {5}
speed   =  4 + energy / 8
```

So there is no training grind competing with looking after the pet — a fed,
rested, clean pet simply wins more. Every roll happens inside the capsule, so
the model narrating the fight cannot nudge its outcome. A round costs energy,
which is a cooldown that needs no timer, and losing costs nothing but that
energy: nothing is ever injured or lost.

### A game the agent cannot cheat at

`pet_game_start` picks a number between 1 and 20 and stores it in the capsule's KV — *inside the
sandbox*. The model sees only the hints, never the secret, so it has to genuinely play:

```
guess 10  ->  Lower than 10 (burning hot). 5 guesses left.
guess  5  ->  Higher than 5 (warm). 4 guesses left.
guess  7  ->  Higher than 7 (burning hot). 3 guesses left.
guess  8  ->  8 it is — guessed in 4! Rex is delighted (+18 happiness).
```

Six attempts; the reward shrinks the longer you take and a round costs energy, so it cannot be farmed.
The secret comes from real host entropy (`astrid:sys.random-bytes`), never the clock — a
clock-derived number would be reproducible by anything that can read the time.

`python3 tools/pet.py autoplay` binary-searches a round automatically, which doubles as proof the
hints are honest.

## Install

### From a release artifact (no Rust needed)

```sh
aos capsule install ./aos-pet.capsule -y
aos agent modify default --add-capsule aos-pet
```

### From source

Requires the `wasm32-unknown-unknown` target; the pinned toolchain installs itself.

```sh
git clone https://github.com/KruGoL/aos-pet.git
cd aos-pet
aos capsule build
aos capsule install ./dist/aos-pet.capsule -y
aos agent modify default --add-capsule aos-pet
```

### Configuration

| Key | Default | Meaning |
|---|---|---|
| `decay_scale` | `1.0` | Time multiplier. `1.0` = real time (a real tamagotchi pace); `60` makes one minute count as an hour — handy for demos. |

```sh
aos capsule config aos-pet --set decay_scale=60
```

## Playing without an LLM

`tools/pet.py` talks to the capsule directly over MCP, so it needs no model and no API
key. It auto-starts the Astrid daemon and answers the broker's one-time ingress consent.

```sh
python3 tools/pet.py status
python3 tools/pet.py feed
python3 tools/pet.py watch 30      # animated
python3 tools/pet.py adopt Rex --replace
```

Requires the `aos-mcp` broker capsule (from
[unicity-aos/oracles](https://github.com/unicity-aos/oracles)) to be installed and
granted — the CE distribution does not ship it.

### Always-visible pet in your terminal

The pet lives in the **tmux status bar**, which tmux draws *outside* the pane — so it
stays visible even while a full-screen TUI like `aos chat` owns the terminal.

**1.** Start the poller. It holds one MCP session open and refreshes `~/.pet-line`, so the
status bar reads a plain file in ~0.1 s instead of spawning `aos mcp serve` every tick:

```sh
python3 tools/pet.py daemon 10 &
```

**2.** Add to `~/.tmux.conf`:

```tmux
set -g status-interval 5
set -g status-right "#(cat ~/.pet-line) | %H:%M"
set -g status-right-length 80
```

**3. Run your agent *inside* tmux — this is the step people miss:**

```sh
tmux new-session 'aos chat'
```

Launching `aos chat` in a plain terminal shows no pet, because there is no tmux status bar
for it to live in. The pet then sits in the bottom-right corner of every tmux window:

```
[0] 0:astrid*                            Rex (-.-) f49 h24 e0 c0 | 16:37
```

`python3 tools/pet.py tmux` prints this setup at any time.

**Want the whole pet in the corner instead of one line?** tmux 3.0+ supports a
multi-row status bar, so the poller also keeps a four-line layout in
`~/.pet-lines`:

```
[0] 0:astrid*
    /\_/\        Мурзик · content · 2d
   ( o.o )       f [##########] 100   h [########--]  80
    > _ <        e [######----]  60   c [##########] 100
                 * ill — needs healing
```

```tmux
set -g status 5
set -g status-interval 2
set -g status-format[1] "#(sed -n 1p ~/.pet-lines)"
set -g status-format[2] "#(sed -n 2p ~/.pet-lines)"
set -g status-format[3] "#(sed -n 3p ~/.pet-lines)"
set -g status-format[4] "#(sed -n 4p ~/.pet-lines)"
```

`status-format[0]` is left alone so the window list stays put, and the face
alternates on every poll — it blinks at you from the corner. It costs four rows
in every window, which is the honest trade.

Prefer a full animated pet to a one-liner? Split the window instead:

```sh
tmux split-window -h "python3 tools/pet.py watch 99999"
```

The compact line is rendered by the capsule itself, so every client shows the pet
identically.

## Development

Pure game logic (decay, moods, art, rendering) lives in SDK-free modules, so it is
testable on the host with no WASM and no running AOS:

```sh
# .cargo/config.toml forces wasm32, so tests need an explicit host target
cargo test --target x86_64-unknown-linux-gnu
```

39 unit tests cover decay over long absences, clocks moving backwards, edge-triggered
alerts, illness onset and cure, mood boundaries, bar rendering and state migration.

```sh
aos capsule check      # validates macro ↔ manifest wiring
aos capsule build
```

`DESIGN.md` documents the full design, the verified platform contracts, and the traps
found along the way — including one design that had to be dropped because the ABI it
assumed does not exist.

## Layout

| Path | Purpose |
|---|---|
| `src/lib.rs` | Capsule entry points: tools, watchdog tick, prompt-injection hook |
| `src/model.rs` | Pet state, alerts, serde |
| `src/decay.rs` | Time-based decay and illness rules |
| `src/mood.rs` | Mood derivation |
| `src/art.rs` | ASCII frames per mood |
| `src/render.rs` | Bars, display, compact line, prompt briefing |
| `Capsule.toml` | Manifest: capabilities and the IPC ACL |
| `tools/pet.py` | MCP client, animated viewer, status-bar poller |

## License

MIT OR Apache-2.0
