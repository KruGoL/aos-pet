# Changelog

## 0.2.0 — 2026-07-23

The audit release. Three independent multi-agent audit rounds ran over 0.1.0;
everything confirmed was fixed, and the fixes themselves were re-audited.

### The big ones

- **Real-time decay actually happens.** Stats now carry fractions (f64)
  end to end. With integer stats, the kernel watchdog's 5-second polling
  rounded every span's decay back to zero while still consuming the elapsed
  time — at real-time scale the pet never aged at all.
- **Time is billed in segments.** `advance` splits a span at the boundaries
  where the rates in force change — the pet dozing off or waking, a physical
  ailment setting in — each found in closed form, never by iterating over
  time. Illness, mood and energy no longer depend on how often the pet
  happened to be polled, and putting it to sleep before an absence no longer
  discounts the whole absence.
- **Illness has a floor and a ceiling.** One `sick` boolean became three
  ailments (famine / grime / gloom), each earned by a different neglect and
  cured a different way; medicine eases but does not replace care. Neglect
  clocks charge only the time a bar actually spent past its threshold, and
  cap at three times onset so no pet is ever beyond saving.
- **Battles express care.** Opponents have fixed stat lines per archetype
  instead of mirroring the player, damage variance scales with attack, and a
  deterministic simulation in the test suite enforces the win-rate bands:
  care shifts the odds against every opponent, the yard dog stays a wall,
  nothing is hopeless.
- **One clock for all fun.** Play, games and battles share a readiness clock;
  no reward can be farmed by alternating them, and no happiness source is
  left unclocked.

### Robustness

- A corrupt or newer-versioned saved record no longer bricks the capsule:
  every tool names the way out, and `pet_adopt replace=true` parks the old
  bytes under `pet.corrupt` instead of destroying them.
- Control characters are stripped from pet names.
- An abandoned guessing game no longer survives into the next pet.
- The Python client survives broker restarts, renders hostile or older JSON,
  and reports startup failures in one line instead of a traceback.

## 0.1.0 — 2026-07-21

Initial release: care loop (feed / play / clean / sleep / heal), lazy decay,
watchdog self-wake with alerts, ambient prompt briefing, rare moments,
autonomy (auto-sleep, self-amusement), guessing game with a sandboxed secret,
NPC battles, rename, tall animated tmux status.
