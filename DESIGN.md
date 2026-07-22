# `aos-pet` — Design & Implementation Plan

A virtual pet capsule for Unicity AOS. The capsule is the **authoritative referee**: pet
state lives in sandboxed, principal-scoped KV and decays by real wall-clock time, so the
LLM can neither fake it nor freeze it.

Status: design approved-pending. Nothing implemented yet.

---

## 1. Why this is interesting (not just a toy)

An LLM can hallucinate state ("your pet is fine!"). A capsule cannot: the numbers live
inside the sandbox, the capsule applies the rules, and every mutation goes through a
typed tool call. This makes the pet an honest demonstration of the whole platform —
persistent state, real time, capability boundaries, and event-driven wake-ups.

## 2. Product decisions (settled)

| Decision | Choice |
|---|---|
| Stakes | Pet **gets sick when neglected but never dies** — always curable |
| Language | All tool names, descriptions and output in **English** |
| Visuals | **ASCII art** per mood, returned by the capsule; optional animated console viewer |
| Backend / DB | **None.** Built-in KV is the entire persistence layer |
| Scope | Full implementation, including self-wake notifications |

## 3. Grounded SDK facts

Every API below was verified in source — no assumptions.

| Need | API | Source |
|---|---|---|
| Wall-clock time | `time::now() -> Result<SystemTime, SysError>` (host `clock_ms`) | `astrid-sdk/src/lib.rs:361` |
| Monotonic (intra-call only) | `time::monotonic() -> Duration` | `lib.rs:371` |
| Persist state | `kv::set_json` / `kv::get_json_opt` | `kv.rs:50` / `kv.rs:42` |
| Schema migration | `kv::set_versioned` / `kv::get_versioned_or_migrate` | `kv.rs:166` / `kv.rs:271` |
| Race-free update | `kv::cas(key, expected, new)` | `kv.rs:111` |
| Self wake-up | subscribe `astrid.v1.watchdog.tick` | `aos-ce/capsules/capsule-react/Capsule.toml:66` |
| Tick handler shape | `#[astrid::interceptor("handle_watchdog_tick")]` | `capsule-react/src/lib.rs:611` |
| Push notification | `uplink` module, `Profile::Notify` ("one-way notification sink") | `lib.rs:236` |
| Nag via system prompt | `allow_prompt_injection` + `prompt_builder.v1.hook.before_build` | `capsule-memory/Capsule.toml:15,21` |
| KV capability form | `[capabilities] kv = []` | `oracles/crates/aos-mcp/Capsule.toml:17` |
| Randomness | `getrandom` custom backend → `astrid:sys.random-bytes` | `astrid-sdk/Cargo.toml:21-25` |

**No `cron` module exists** in the SDK (the README mentions one; `lib.rs` does not have it).
Periodic behaviour therefore comes from the kernel watchdog tick, not a timer module.

## 4. Core mechanic — lazy decay

The capsule only runs when invoked. Instead of ticking, it stores `last_seen_ms` and
recomputes on every entry point:

```
elapsed      = now() - last_seen
fullness    -= rate_fullness    * elapsed
happiness   -= rate_happiness   * elapsed
cleanliness -= rate_cleanliness * elapsed
energy      -= rate_energy      * elapsed      // but RISES while sleeping
clamp all to 0..=100
last_seen    = now()
```

Consequences (all desirable):
- The pet keeps living while the daemon is stopped or the machine is asleep — time is
  measured by the clock, not by ticks.
- You cannot pause the pet by shutting AOS down.
- Decay is pure arithmetic → trivially unit-testable on the host with no WASM and no AOS.

Rates are configurable via `[env]` (points per hour) plus a `decay_scale` multiplier so a
demo can run in minutes while a real deployment runs in hours.

## 5. State model

KV key `pet`, one pet per principal (KV is already principal-scoped → per-user pets for free).
Written with `set_versioned(..., version = 1)`.

```rust
struct Pet {
    name: String,
    born_at_ms: u64,
    last_seen_ms: u64,
    fullness: u8,        // 0..=100, 100 = well fed
    happiness: u8,       // 0..=100
    energy: u8,          // 0..=100
    cleanliness: u8,     // 0..=100
    sleeping: bool,
    sick: bool,
    neglect_ms: u64,     // accumulates while any stat sits at 0
    alerts: Vec<Alert>,  // capped ring of recent events
    last_alert_ms: u64,  // notification cooldown
}

struct Alert { at_ms: u64, kind: AlertKind, message: String }
```

All stats use "higher is better" semantics so bars render uniformly.

### Sickness rule (no death)
- While any stat is `0`, `neglect_ms` accumulates.
- `neglect_ms > sick_after` → `sick = true`.
- Sick effects: happiness decays faster; `pet_play` refuses ("too sick to play").
- `pet_heal` clears `sick`, resets `neglect_ms`, small stat boost.
- **The pet never dies.** There is no terminal state.

### Mood (derived, drives the art)
```
sleeping                -> Sleeping
sick                    -> Sick
fullness    < 25        -> Hungry
energy      < 25        -> Tired
cleanliness < 25        -> Dirty
happiness   < 25        -> Sad
all >= 70               -> Happy
otherwise               -> Neutral
```

## 6. Tool surface (English)

| Tool | Args | Behaviour |
|---|---|---|
| `pet_adopt` | `name` | Create a pet (refuses if one exists unless `replace: true`) |
| `pet_status` | — | Art + stat bars + mood + age. Read-only apart from decay catch-up |
| `pet_feed` | — | `fullness += FEED`, tiny happiness bump |
| `pet_play` | — | `happiness += PLAY`, `energy -= COST`; refuses if sick or sleeping |
| `pet_sleep` | `wake: bool?` | Toggle sleeping; while asleep energy rises and other decay slows |
| `pet_clean` | — | `cleanliness = 100` |
| `pet_heal` | — | Cure sickness |
| `pet_alerts` | — | Recent recorded events ("got hungry at 14:32") |

Every tool returns a struct serialised to JSON containing both:
- `display` — pre-rendered string (ASCII art + bars) for humans/agents, and
- raw fields (`mood`, stats, `frames`) so a viewer can render its own animation.

Tool doc-comments become the descriptions the LLM reads, so they are written for the model.

## 7. Self-wake notifications — VERIFIED, and one design corrected

An adversarially-verified source investigation overturned part of the original plan.
Recorded here because the mistake is an easy one to repeat.

### 7.1 Kernel tick — IMPLEMENTED and proven

The kernel emits `astrid.v1.watchdog.tick` every **5 seconds**
(`astrid-kernel/src/lib.rs:3103-3127`, `interval(Duration::from_secs(5))`, first tick
skipped). Any capsule may subscribe: **no capability and no publish row are required.**

```toml
[subscribe]
"astrid.v1.watchdog.tick" = { wit = "@unicity-astrid/wit/system/watchdog-tick", handler = "handle_watchdog_tick" }
```

⚠️ **The handler must take NO argument.** The WIT record declares `timestamp-ms: u64`,
but the kernel actually publishes `{}`. A handler with a required payload field fails
`serde_json::from_slice` and is denied at runtime. `capsule-react` takes no argument for
exactly this reason.

The tick handler skips the KV write when nothing observable changed. That is not only a
write-reduction: leaving `last_seen` untouched makes the next tick measure one full span
instead of rounding many sub-unit slices down to zero.

**Verified live:** adopted a pet, waited 30 s at `decay_scale=2000` without calling
anything, and the capsule had autonomously recorded four alerts.

### 7.2 Uplink push — DOES NOT EXIST. Do not implement.

The original plan said "`uplink` capability, `Profile::Notify`, to deliver a message".
Every part of that is wrong:

- **`uplink::send()` does the opposite of its own doc-comment.** The doc says "send a
  message to a user"; the WIT, the host implementation and the book all say it injects an
  **inbound** message — it simulates the user typing. Treat the doc-comment as a known bug.
- **It is dead code in CE.** `inbound_tx` only exists when a manifest declares `[[uplink]]`,
  and **zero** manifests in the tree do. Every call returns `Err(Quota)`.
- **`Profile::Notify` is not a push channel** despite the name — the WIT calls it a
  "one-way notification sink", i.e. ingress.
- **There is no proactive outbound push ABI anywhere.** Grepping `astrid-wit/interfaces`
  for a notification interface returns nothing.

The only thing that renders text on a user's frontend is publishing `agent.v1.response`
with the exact `session_id` the frontend is bound to (captured from a real
`user.v1.prompt`; a wrong id is dropped silently). Even then it works on `aos-cli` but is
**silently discarded by `aos-telegram`**, whose handler only processes responses while a
turn is active.

Decision: **not implemented.** A notification path that silently does nothing is worse
than none. The tick + `pet_alerts` covers the need honestly.

### 7.3 Prompt injection — IMPLEMENTED

```toml
[capabilities]
allow_prompt_injection = true          # without this the text is silently stripped

[publish]
"prompt_builder.v1.hook_response.*" = { wit = "@unicity-astrid/wit/prompt/before-build-hook-response" }

[subscribe]
"prompt_builder.v1.hook.before_build" = { wit = "@unicity-astrid/wit/prompt/before-build-hook", handler = "on_before_prompt_build", priority = 90 }
```

Contract traps, all verified:
- **Case asymmetry.** The inbound payload is snake_case (`response_topic`,
  `system_prompt`); the response fields are camelCase (`appendSystemContext`,
  `prependSystemContext`, `systemPrompt`, `prependContext`). Sending snake_case parses to
  an all-`None` response and is discarded with no error and no log on the sender's side.
- **The return value is ignored.** You must `ipc::publish_json(response_topic, …)`.
- **250 ms cliff.** The builder stops collecting once `HOOK_FIRST_RESPONSE_MS` elapses, so
  the handler does a KV read and arithmetic only — no I/O, and no save.
- **Never construct the reply topic.** Always read `response_topic` from the payload.
- `priority` on a handler-less subscribe row is a hard parse error, not a warning.

This activates the moment an LLM provider is configured; it cannot be demonstrated on a
bench with no provider key.

## 7a. Frontend portability — the pet is channel-agnostic

The pet capsule never talks to a frontend. It exposes tools on the typed bus, and the
kernel routes. Any current or future frontend therefore sees the pet with **zero changes
to this capsule**.

`aos-telegram` already exists at `aos-ce/capsules/capsule-telegram/` (built but excluded
from the 19-capsule CE release). Its manifest shows it is a pure uplink: it publishes
`user.v1.prompt` and subscribes `agent.v1.response` / `agent.v1.stream.delta` — it knows
nothing about tools.

```
Telegram → aos-telegram → user.v1.prompt → aos-react
                                             → aos-router → aos-pet (tool.v1.execute.pet_status)
         ← agent.v1.response ←───────────── ← art + stats
```

Implications:
- The same pet is reachable from CLI, MCP (already proven), Telegram, or a future web UI.
- **Per-user pets come free**: KV is principal-scoped, so if the frontend assigns each
  external user their own principal (what `aos-users` exists for), each person gets their
  own pet. A single shared principal means a single shared pet.
- The conversational path through any chat frontend still needs an LLM provider, because
  the agent is what decides to call the tool. Direct MCP calls do not.
- ASCII art must survive proportional fonts — render inside a code block where the
  frontend supports it.

## 7b. Playing from a plain terminal — no LLM required

The console is a first-class channel: `aos-cli` is already installed and is exactly the
frontend that bridges the `aos` command surface to the kernel bus. What the bench lacks is
a *brain* (an LLM provider), and none of the paths below need one.

| Path | Needs LLM | Needs extra frontend |
|---|---|---|
| `aos pet status` — native CLI verb via `[[command]]` | no | no |
| Direct MCP `tools/call` (proven working on `hello`) | no | no |
| Animated viewer polling either of the above | no | no |
| `aos chat` → "feed my pet" (agent decides) | **yes** | no |

A capsule registers its own terminal verb with `[[command]]`. Shipped precedents:
`capsule-registry` (`models`), `capsule-session` (`session`), `capsule-identity`
(`identity-export` / `identity-import`).

⚠️ **`kind` is not optional in practice.** A `[[command]]` with no `kind` defaults to
`Slash` — a TUI `/verb`, **not** a terminal verb. Of all 21 first-party aos-ce capsules
only `capsule-registry` (`models`) declares `kind = "cli"`; `capsule-session` and
`capsule-identity` are slash commands despite superficially looking like CLI verbs.

A real terminal verb needs more than a manifest line — it is **not** a `handler =`
binding:

```toml
[[command]]
name = "pet"                 # grammar: [a-z][a-z0-9-]*, 1..32, not a reserved built-in
kind = "cli"
description = "Care for your virtual pet"

[subscribe]
"cli.v1.command.run.aos-pet" = { wit = "opaque" }   # ACL only — no handler key

[publish]
"cli.v1.command.result.*" = { wit = "opaque" }
```

The capsule must run an `#[astrid::run]` loop that dynamically subscribes to
`cli.v1.command.run.<package>`, reads `{req_id, command, args[]}`, and replies on
`cli.v1.command.result.<req_id>` with `{req_id, exit_code, output, error?}`. The kernel
routes but does not interpret the payload. `output` goes to stdout, `error` to stderr,
`exit_code` becomes the process exit status. No capability is required. Invocable as
`aos pet …`, `aos capsule pet …`, or `aos capsule run aos-pet pet …`.

**Status: deliberately deferred.** Adding a run loop changes the capsule's entrypoint
architecture — the shipped fleet uses *either* interceptors *or* a single `#[astrid::run]`
— and would risk the working eight-tool interceptor surface for ergonomics we already have
via `tools/pet.py`. The recipe above is recorded so it can be added as a follow-up with
its own verification.

## 8. Manifest sketch

```toml
[package]
name = "aos-pet"
version = "0.1.0"
description = "A virtual pet whose state decays in real time"
astrid-version = ">=0.7.0"

[[component]]
id = "aos-pet"
file = "aos_pet.wasm"
type = "executable"

[capabilities]
kv = []
uplink = true
allow_prompt_injection = true

[env]
decay_scale = { type = "string", default = "1.0", request = "Decay speed multiplier (1.0 = real time)" }

# Native terminal verb — lets the user play with no LLM: `aos pet status`, `aos pet feed`
[[command]]
name = "pet"
kind = "cli"
description = "Care for your virtual pet: status, feed, play, sleep, clean, heal"

[publish]
"tool.v1.execute.*.result"          = { wit = "@unicity-astrid/wit/types/tool-call-result" }
"tool.v1.response.describe.*"       = { wit = "@unicity-astrid/wit/tool/describe-response" }
"prompt_builder.v1.hook_response.*" = { wit = "@unicity-astrid/wit/prompt/before-build-hook-response" }

[subscribe]
"tool.v1.execute.pet_adopt"   = { wit = "@unicity-astrid/wit/types/tool-call", handler = "tool_execute_pet_adopt" }
# ... one concrete row per tool (wildcards do NOT dispatch handlers) ...
"tool.v1.request.describe"    = { wit = "@unicity-astrid/wit/tool/describe-request", handler = "tool_describe" }
"astrid.v1.watchdog.tick"     = { wit = "@unicity-astrid/wit/system/watchdog-tick", handler = "handle_watchdog_tick" }
"prompt_builder.v1.hook.before_build" = { wit = "@unicity-astrid/wit/prompt/before-build-hook", handler = "on_before_prompt_build" }
```

Note: every tool needs its **own concrete** `tool.v1.execute.<name>` row — a wildcard
authorises but does not dispatch to a handler.

## 9. File layout

```
aos-pet/
├── .cargo/config.toml     # wasm32-unknown-unknown + getrandom custom backend
├── rust-toolchain.toml
├── Cargo.toml             # cdylib, astrid-sdk 0.7 + serde + serde_json
├── Capsule.toml
├── src/
│   ├── lib.rs             # #[capsule] impl — thin handlers only
│   ├── model.rs           # Pet, Alert, serde, versioned load/save
│   ├── decay.rs           # pure decay + sickness math  (+ tests)
│   ├── mood.rs            # mood derivation             (+ tests)
│   ├── art.rs             # ASCII frames per mood (2 frames each, for animation)
│   └── render.rs          # bars + display string       (+ tests)
└── tools/pet-viewer.py    # optional animated console viewer
```

Handlers stay thin; all rules live in pure, host-testable functions — matching the
discipline used across the shipped fleet.

## 10. Testing strategy

**Unit (host, `cargo test`, no WASM/AOS):**
- decay over various elapsed spans, including very long gaps and clamping at 0/100
- energy rises while sleeping; other stats decay slower
- sickness onset after sustained neglect, and cure via `pet_heal`
- mood selection at every boundary
- alert dedup / cooldown behaviour
- versioned state round-trip and migration from unversioned data
- bar rendering widths

**Wiring:** `aos capsule check` (macro ↔ manifest), then `capsule_doctor` after install.

**Live:** install, then drive each tool through the Python MCP client already built
(`scratchpad/mcp_client.py`), which auto-accepts the ingress consent.

## 11. Implementation phases

1. **Skeleton** — `aos capsule new aos-pet`, manifest, `model.rs`, KV persistence,
   `pet_adopt` + `pet_status`. Build, install, call live.
2. **Mechanics** — decay, all actions, sickness/heal, with unit tests.
3. **Presentation** — `art.rs` + `render.rs`, pretty output with bars.
4. **Self-wake** — watchdog tick handler, alerts, cooldown, `pet_alerts`.
5. **Integration** — uplink notify + prompt injection (wired, partially demonstrable).
6. **Viewer** — animated console viewer script.

Each phase ends with a green `cargo test` and a successful build+install.

## 11a. Implementation status

Built, installed and played live on AOS 2026.1.3 (WSL Ubuntu 24.04).

| Phase | State | Evidence |
|---|---|---|
| 1–3 core game | **done** | 37 host unit tests green; `capsule check` "8 tool(s), all wired"; adopted/fed/played live |
| 4 tick alerts | **done** | 30 s untouched at `decay_scale=2000` → four alerts recorded autonomously |
| 5 prompt injection | **wired** | manifest + handler in place; needs an LLM provider to observe |
| 5 uplink push | **dropped** | no such ABI exists — see §7.2 |
| 5 CLI verb | **deferred** | needs a run-loop refactor — see §7b |
| 6 viewer | **done** | `tools/pet.py` incl. `watch` animated mode, self-healing daemon |

Verified extras worth remembering:
- Host unit tests need an explicit target — `.cargo/config.toml` forces wasm32, so use
  `cargo test --target x86_64-unknown-linux-gnu`. `astrid-sdk` does compile for the host.
- `aos capsule install` hot-reloads into a running daemon: "Live: the running daemon
  loaded 'aos-pet' — no restart needed."
- Astrid binds a daemon to a *workspace*; running `mcp serve` from a different cwd fails
  with "daemon belongs to another project". `tools/pet.py` pins every `aos` call to `$HOME`
  and restarts once if it still mismatches.

## 12. Open items to verify during implementation

All resolved by a 6-question, adversarially-verified source investigation:

| Question | Answer |
|---|---|
| Tick WIT / payload / period | `@unicity-astrid/wit/system/watchdog-tick`; kernel publishes `{}` (not the declared `timestamp-ms`); every 5 s |
| May any capsule subscribe to the tick? | Yes — no capability, no publish row |
| `uplink` capability spelling | bare boolean `uplink = true` — but we need neither it nor `[[uplink]]` |
| Proactive push through another frontend | **No such ABI.** Only `agent.v1.response` with an exact `session_id`, silently dropped by Telegram. Dropped from scope |
| Building on `/mnt/c` | Not a problem: scaffold 0.5 s, release build ~29 s. `CARGO_TARGET_DIR` never needed |
| Running host unit tests for a cdylib | `cargo test --target x86_64-unknown-linux-gnu`; the SDK compiles for the host |

Remaining genuinely open:
- Whether `#[astrid::run]` can coexist with `#[astrid::tool]` interceptors in one capsule.
  This gates the native `aos pet` verb (§7b) and needs an experiment, not a code read.

## 13. Build location

Source lives at `c:/work/unicity/aos/aos-pet` (visible in VSCode, git-able), built from
WSL via `/mnt/c/work/unicity/aos/aos-pet`. `hello` compiled in ~20 s, so this is expected
to be acceptable; if it is not, move the crate into the WSL-native filesystem and sync.
