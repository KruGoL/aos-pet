# Authoring `aos-pet`

This is a complete, from-zero guide to writing this Astrid capsule. Read it
before editing anything; you should not need Astrid's own source to be
productive.

## What a capsule is

A **capsule** is a WebAssembly component (`wasm32-unknown-unknown`) that runs
inside the Astrid kernel's sandbox. It cannot make raw syscalls or touch the
network/filesystem directly — every effect goes through the audited
`astrid:*` host surface exposed by `astrid-sdk`, and every effect is gated by a
capability you declare in `Capsule.toml`. A **tool capsule** (this one) exposes
one or more typed *tools* that the agent's LLM can call.

Communication is exclusively over the kernel's IPC event bus: the capsule
*subscribes* to a tool-execution topic and *publishes* a result. There are no
function imports from the kernel and no shared memory — just typed messages on
named topics, and an ACL (declared in the manifest) that says which topics this
capsule may touch.

## The tool pattern

A tool is a method on your capsule struct, inside a `#[capsule]` impl block,
annotated with `#[astrid::tool("name")]`:

```rust
use astrid_sdk::prelude::*;
use astrid_sdk::schemars;
use serde::Deserialize;

// The argument type. It must derive `Deserialize` (the kernel hands you the
// call arguments as JSON) and `schemars::JsonSchema` (the SDK turns this into
// the tool's input schema so the LLM knows how to call it). Doc comments on
// fields become the field descriptions the model sees.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct GreetArgs {
    /// Who to greet.
    pub name: String,
}

#[derive(Default)]
pub struct Capsule;

#[capsule]
impl Capsule {
    /// Greet someone by name. This doc comment becomes the tool description
    /// shown to the LLM.
    #[astrid::tool("greet")]
    pub fn greet(&self, args: GreetArgs) -> Result<String, SysError> {
        Ok(format!("Hello, {}!", args.name.trim()))
    }
}
```

Rules that matter:

- The impl block carries `#[capsule]`. That macro generates the WASM component
  glue (the `Guest` impl and the component export) for you — you never write it
  by hand.
- Each tool method takes `&self` and exactly one argument struct, and returns
  `Result<T, SysError>` where `T` serializes to JSON (a `String`, or any
  `serde::Serialize` type). Return `Err(SysError…)` to signal a failure to the
  agent.
- The macro auto-generates a `tool_describe` export that reports every tool's
  JSON schema. You do **not** write a describe handler — declaring the tools is
  enough.
- A tool that mutates state (writes a file, sends a request that changes
  something) should be marked `#[astrid::tool("name", mutable)]`. A pure /
  read-only tool omits it. The `mutable` flag is what lets the runtime treat the
  call as side-effecting (e.g. for approval gating).

### Wiring a new tool

Adding a tool is two edits — the code above, plus a `[subscribe]` line in
`Capsule.toml` that routes the tool's bus topic to the generated handler. For a
tool named `greet`, the synthetic handler name is `tool_execute_greet`:

```toml
[subscribe]
"tool.v1.execute.greet" = { wit = "@unicity-astrid/wit/types/tool-call", handler = "tool_execute_greet" }
```

The handler name is always `tool_execute_<tool_name>`. Forgetting the
`[subscribe]` line means the kernel never routes the call to you — the tool is
defined but unreachable.

## `Capsule.toml` — the manifest and the ACL

The manifest is **untrusted input** the kernel parses and enforces. It has four
parts that matter to a tool capsule.

### `[package]` and `[[component]]`

```toml
[package]
name = "aos-pet"
version = "0.1.0"
description = "An Astrid tool capsule."
astrid-version = ">=0.7.0"

[[component]]
id = "aos-pet"
file = "aos_pet.wasm"
type = "executable"
```

`file` must be the **crate name with hyphens turned into underscores**, plus
`.wasm` — that is exactly what `cargo` names the `cdylib` output. Get this wrong
and the installer cannot find the built artifact.

### `[capabilities]` — what the kernel will let you do

Every host effect is gated here, and the model is **fail-closed**: if a
capability field is absent, the kernel treats it as an *empty allowlist*, i.e.
deny-all — not "unconfigured, allow through". Grant only what your tools
actually use. The capability keys a tool capsule cares about:

| Key | Grants |
|-----|--------|
| `net` | Outbound HTTP to a hostname allowlist, e.g. `net = ["api.example.com"]` (or `["*"]` for any). |
| `fs_read` | VFS read paths, e.g. `fs_read = ["home://"]`. |
| `fs_write` | VFS write paths, e.g. `fs_write = ["home://output/"]`. |
| `net_connect` | Outbound TCP `host:port` allowlist. |
| `host_process` | Host-process command allowlist. |
| `identity` | Identity operations (`resolve` / `link` / `admin`). |
| `kv` | Key–value store access; `kv = []` grants the capsule its own principal-scoped namespace. |
| `allow_prompt_injection` | Lets the prompt-builder hook actually inject text; without it the response is silently stripped. |

This capsule's own `Capsule.toml` is a live example: it declares `kv = []` for
the pet state and `allow_prompt_injection = true` for the ambient briefing, and
nothing else. Grant only what a tool actually uses — and expect a hard denial
at runtime if you forget one.

### `[publish]` / `[subscribe]` — the IPC ACL

These two tables are the *only* declaration of which bus topics the capsule may
touch, and they double as the ACL the kernel enforces. Empty tables = the
capsule can neither send nor receive anything (fail closed). A tool capsule
needs exactly this shape:

```toml
[publish]
"tool.v1.execute.*.result"    = { wit = "@unicity-astrid/wit/types/tool-call-result" }
"tool.v1.response.describe.*" = { wit = "@unicity-astrid/wit/tool/describe-response" }

[subscribe]
"tool.v1.execute.greet"    = { wit = "@unicity-astrid/wit/types/tool-call", handler = "tool_execute_greet" }
"tool.v1.request.describe" = { wit = "@unicity-astrid/wit/tool/describe-request", handler = "tool_describe" }
```

- A `[subscribe]` entry's `handler` binds the topic to a generated WASM export.
  `tool.v1.execute.greet` → `tool_execute_greet`; the describe fan-out request
  → the auto-generated `tool_describe`.
- The `[publish]` entries authorize sending the per-tool result
  (`tool.v1.execute.<name>.result`) and the schema response. The trailing `*`
  in a publish/subscribe ACL key is a **subtree** wildcard — `tool.v1.execute.*.result`
  authorizes the result topic for any tool name.
- The `wit = "@unicity-astrid/wit/..."` reference names the typed payload schema
  for that topic, resolved from the shared SDK contracts at build time. Use the
  references shown above verbatim for the tool-bus topics.

The tool-bus topic conventions, for reference:

| Topic | Meaning |
|-------|---------|
| `tool.v1.execute.<name>` | The kernel dispatches a call of tool `<name>` to you (subscribe). |
| `tool.v1.execute.<name>.result` | You publish the tool's result here. |
| `tool.v1.request.describe` | Schema fan-out request (subscribe; handled by `tool_describe`). |
| `tool.v1.response.describe.*` | You publish your schema response here. |

## The getrandom footgun (already handled)

This scaffold already writes the fix, so you do not have to — but know why it is
there. On `wasm32-unknown-unknown`, the `getrandom` crate (pulled in
transitively by `uuid` v4 and `HashMap`'s random seeding) **refuses to link**
without an explicit backend. `.cargo/config.toml` sets it:

```toml
[target.wasm32-unknown-unknown]
rustflags = ["--cfg=getrandom_backend=\"custom\""]
```

`astrid-sdk` provides the custom backend symbol (it routes to the kernel's
audited CSPRNG). If you ever see an opaque link error mentioning `getrandom`
when building a capsule, a missing or mangled version of this cfg is the cause.
Do not remove it.

## WIT / interfaces — just enough

The bus payloads are typed by WIT records. As a tool author you only deal with
four, all referenced from your manifest (you never hand-write their
definitions):

- `tool-call` — the incoming call (tool name + JSON arguments).
- `tool-call-result` — the result you publish back.
- `describe-request` / `describe-response` — the schema fan-out the SDK handles
  for you via `tool_describe`.

The `wit/` directory is **generated at build time** and embedded in the
`.capsule` archive — do not hand-write it and do not commit it.

## The dev loop: build → install → call

```sh
# 1. Build and package. `aos capsule build` reads .cargo/config.toml for the
#    target, compiles to wasm32-unknown-unknown, stages the wit/, and packs a
#    .capsule archive under dist/. (A plain `cargo build` produces only the raw
#    .wasm — use `aos capsule build` to get an installable archive.)
aos capsule build

# 2. Install into the running daemon.
aos capsule install ./dist/aos-pet.capsule

# 3. The tools are now discoverable. Once installed, the capsule's tools are
#    described to the agent automatically (via tool_describe), so the LLM can
#    call them. Iterate: edit, rebuild, reinstall.
```

That is the whole loop. Edit `src/lib.rs`, declare each tool's `[subscribe]`
line, grant the capabilities the tool needs, rebuild, reinstall.
