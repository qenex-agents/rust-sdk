# QENEX seat binding

The QENEX platform gives every bot a private Linux desktop and a private working
tree on a seat host. This crate is the **seat runtime**: an MCP server that
exposes exactly one seat's execution surface, so a bot's agent can build and
run developer tools from scratch inside its own workspace without reaching
another bot's state or the host's global state.

```
crates/qenex-seat-runtime   # this crate
crates/rmcp                 # the MCP SDK it is built on
```

## Running it

The runtime binds one seat at startup and refuses to run without one:

```sh
QENEX_SEAT=coder \
QENEX_SEAT_WORKSPACE=/var/lib/qenex/seats/coder/workspace \
cargo run -p qenex-seat-runtime
```

It speaks MCP over stdio, so an MCP client launches it as a child process and
talks to it over stdin/stdout.

## Environment contract

| Variable | Meaning |
| --- | --- |
| `QENEX_SEAT` | Seat identifier, for example `coder`. Absent or empty means the runtime refuses to start. |
| `QENEX_SEAT_WORKSPACE` | Absolute path of the seat working tree. It is the only writable root. |
| `QENEX_SEAT_API` | Optional base URL of the seat runtime service. |
| `QENEX_SEAT_TOKEN` | Bearer token for that service. **Never read, logged or written by this crate.** |
| `QENEX_SEAT_ALLOW_GLOBAL=1` | Operator escape hatch that disables the write confinement. |

## Tools

| Tool | Purpose |
| --- | --- |
| `seat_info` | Seat identity, workspace, confinement policy, allowlists and limits. |
| `seat_exec` | Run an allowlisted program in the seat workspace; returns exit code, stdout and stderr. |
| `seat_read_file` | Read a file inside the seat workspace. |
| `seat_write_file` | Write a file inside the seat workspace. |
| `seat_list_dir` | List a directory inside the seat workspace. |

## Rules

- **R1 Detection** - seat-bound iff `QENEX_SEAT` is set and non-empty. A seat without
  `QENEX_SEAT_WORKSPACE` is refused instead of silently falling back to global paths.
- **R2 Confinement** - every path is resolved against the seat workspace. `..`
  traversal, absolute paths and symlink escapes are refused with distinct errors
  (`PathError::Traversal`, `PathError::Absolute`, `PathError::SymlinkEscape`).
- **R3 Reads stay global** - reading the host toolchain is never blocked; only writes
  are confined.
- **R4 Identity is observable** - `seat_info` and every response carry
  `seat=<name>` and the workspace, and child processes inherit
  `QENEX_SEAT` and `QENEX_SEAT_WORKSPACE`.
- **R5 Runtime binding** - `QENEX_SEAT_API` is reported by `seat_info` so a caller can
  discover the service endpoint. Routing calls through it is optional.
- **R6 No new dependencies** beyond the MCP SDK, tokio and serde.
- **R7 Tests** - unit tests cover the unbound, bound and refused states.
- **R9 Surgical diff** - the crate is additive; `crates/rmcp` is untouched.

## Execution safety

- Programs must be in the runtime allowlist; there is no shell, arguments are
  passed as an argv array.
- `git` is limited to read-only subcommands. `commit`, `push`, `reset` and
  friends are refused.
- The default timeout is 120 s and the maximum is 600 s; a timeout kills the
  child process.
- Captured stdout and stderr are capped at 1 MiB, and file reads at 1 MiB.

## Seat host integration

A seat host runs one runtime per seat, for example:

```
QENEX_SEAT=coder
QENEX_SEAT_WORKSPACE=/var/lib/qenex/seats/coder/workspace
```

The runtime is the authoritative execution backend for that seat: the workspace
of the seat is the only place its bots may build, and the only place the seat's
own tools may write.
