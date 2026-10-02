# Config Runtime API

ShojiWM is split into crates. The compositor does not know which language the
user's config is written in: it talks to a **config runtime** through the
traits in `src/shojiwm_lib/src/runtime_api/`. Background: issue #119.

| Crate (dir) | Contents |
|---|---|
| `shojiwm_lib` (`src/shojiwm_lib`) | The compositor, `runtime_api`, the static fallback decoration, `run()`, the global allocator. No V8. |
| `shoji_wm` (`src/shojiwm`) | The TypeScript runtime (embedded V8 via RustyScript/deno_core, the bridge protocol, `TypeScriptLauncher`) and the default `shoji_wm` binary. |
| `shojiwm_rs` (`src/shojiwm_rs`) | Runtime support for configs written in Rust (`RustLauncher`); library only, the config crate builds the binary. |

Other languages (C#, ...) depend on `shojiwm_lib` only and ship their own
binary, so they never link V8.

```text
shoji_wm::run(launcher)
  ├─ parses the common command line (runtime_api::cli)
  ├─ launcher.launch(LaunchContext) ─────────────▶ Box<dyn ConfigRuntime>
  └─ compositor ── request(RuntimeRequest) ──▶ runtime   answered with RuntimeReply
                ── post(RuntimeEvent) ──────▶ runtime   fire and forget
                ◀── RuntimeHost::send(HostMessage) ──   side effects, any thread
```

## Writing a runtime

```rust
use shojiwm_lib::runtime_api::*;

struct MyLauncher;
impl RuntimeLauncher for MyLauncher {
    fn name(&self) -> &'static str { "csharp" }
    fn launch(&self, ctx: LaunchContext) -> Box<dyn ConfigRuntime> {
        Box::new(MyRuntime { host: ctx.host })
    }
}

struct MyRuntime { host: RuntimeHost }
impl ConfigRuntime for MyRuntime {
    fn enable(&mut self) -> Result<(), RuntimeError> {
        // register key bindings, outputs, ... through the host
        Ok(())
    }
    fn request(&mut self, now_ms: f64, request: RuntimeRequest<'_>)
        -> Result<RuntimeReply, RuntimeError>
    {
        Ok(match request {
            RuntimeRequest::Decoration(DecorationRequest::Evaluate { window, .. }) => {
                RuntimeReply::Evaluation(Box::new(/* build a tree */ todo!()))
            }
            _ => RuntimeReply::Unhandled, // built-in behavior
        })
    }
}

fn main() -> std::process::ExitCode { shojiwm_lib::run(MyLauncher) }
```

A Rust config does not need its own launcher: `shojiwm_rs::RustLauncher::new(|ctx| MyRuntime { host: ctx.host })`
wraps any `ConfigRuntime`, and `shojiwm_rs` re-exports `run` and the whole API.

`RuntimeReply::Unhandled` always means "use the compositor's built-in
behavior" (static decoration, server-side decorations, no-op hooks), so a
runtime can start with a handful of requests and grow.

## The pieces

| Item | Role |
|---|---|
| `RuntimeLauncher` | One per language. `name`, `default_config_path`, `extra_args`, `launch`. |
| `ConfigRuntime` | The running config. `preload`, `enable`, `reload`, `request`, `post`, `shutdown`. Called on the compositor thread only. |
| `RuntimeRequest<'a>` | Needs an answer this turn: decoration evaluation (full / cached / policy / handlers / close), scheduler tick, window requests, input hooks, effects, workspace activation. Borrows its snapshots, never serialized. |
| `RuntimeReply` | The direct answer. Side effects are not part of it. |
| `RuntimeEvent` | Fire and forget: display / input / keyboard-layout state, async pointer and gesture hooks. |
| `RuntimeHost` | Clone + Send handle back to the compositor: `send(HostMessage)` and `wake()`. |
| `HostMessage` | Config deltas (outputs, workspaces, key bindings, pointer, input, event filter, processes, debug, cursor, env) and async hook results. |
| `RuntimeConfigDelta` | Helper that publishes a reply's worth of deltas in the canonical order. |
| `RuntimeHandle` | Compositor side: typed wrappers, `Unhandled` fallbacks, de-duplication of posted state. |
| `NullRuntime` | Answers everything with `Unhandled`. |

### Ordering contract

The compositor drains the host queue right after every `request`, before it
acts on the reply. A runtime that `send`s its deltas before returning the
reply therefore gets exactly the old in-band ordering (a key binding update is
active before the reply's window actions run). Messages sent from another
thread are applied on the next drain or on the host's calloop ping, whichever
comes first. `wake()` makes the ping handler run a scheduler tick.

### Threading

`ConfigRuntime` methods are `&mut self` and run on the calloop thread. A
runtime that lives on its own thread (TypeScript isolate, CoreCLR) forwards
each message there and blocks for the answer, as `EmbeddedRuntime` does. A Rust
runtime simply handles the message in place.

### Performance

Requests are plain Rust values. `SchedulerTick` and
`DecorationRequest::EvaluateCached` run every frame while something animates,
so a foreign runtime must not round-trip them through JSON. The TypeScript
runtime turns them into op2 fast calls and a fixed binary layout
(`ssd/embedded_runtime.rs`); an in-process .NET runtime should use C structs
and function pointers the same way. JSON is fine for rare messages (config
load, policy decisions).

### Hot reload

`ConfigRuntime::reload` owns the whole swap. The TypeScript runtime calls
`onDisable`, replaces the isolate and passes the persisted state to `onEnable`;
a .NET runtime can keep the CLR and swap assemblies. On `Ok` the compositor
drops every cached evaluation and asks again; the default implementation
returns `RuntimeError::Unsupported`, shown as a config error.

## Common command line

Every binary built on `shoji_wm::run` accepts the same options; each also has
an environment variable for session wrappers, and the command line wins.

| Option | Env | Meaning |
|---|---|---|
| `--tty` | | DRM/KMS session instead of nested |
| `--tty-output <NAME,...>` | `SHOJI_TTY_OUTPUT` | Only drive these connectors |
| `--config <PATH>` | `SHOJI_CONFIG` | Config entry point (else the launcher's default) |
| `--runtime-dir <DIR>` | `SHOJI_RUNTIME_DIR` | The runtime's own files |
| `--dev` | | Run from a source checkout |
| `--log-off` | `SHOJI_LOG=off` | No session log |
| `--no-log-rotate` | `SHOJI_LOG_ROTATE=off` | Overwrite `latest.log` |
| `--xwayland-satellite-path <PATH>` | `SHOJI_XWAYLAND_SATELLITE_PATH` | External satellite |
| `--xwayland-satellite-glamor <gl\|es\|none>` | `SHOJI_XWAYLAND_SATELLITE_GLAMOR` | Glamor mode |
| `-h`, `--help` / `-V`, `--version` | | |

Runtime-specific options are declared with `RuntimeLauncher::extra_args` and
show up in `--help` under their own heading; the TypeScript runtime adds
`--decoration-runtime <PATH>` (`SHOJI_DECORATION_RUNTIME`).

## Status and next steps

Done:

- `runtime_api` module, message types, `RuntimeHost`, common CLI, `run()`.
- Crate split: `shojiwm_lib` / `shoji_wm` (TS + binary) / `shojiwm_rs`.
- The compositor only talks to `RuntimeHandle`; nothing reaches into the
  TypeScript evaluator any more (`as_embedded` is gone).
- Config deltas left the reply structs and travel as `HostMessage`s; the TS
  wake op uses `RuntimeHost::wake` instead of `SIGUSR1`.

Next:

- A Rust config API in `shojiwm_rs` and an example that ports the current
  TypeScript config (`packages/config`) to Rust.
