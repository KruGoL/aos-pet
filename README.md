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
| `pet_play` | Happiness ↑, energy ↓ — refuses if ill or asleep |
| `pet_sleep` | Sleep / wake (`wake: true`). Energy recovers while asleep |
| `pet_clean` | Cleanliness → 100 |
| `pet_heal` | Cure illness |
| `pet_alerts` | What happened while you were away |

Neglect makes the pet **ill, never dead** — healing always works.

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
