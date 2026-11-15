# Getting Started

## Installation

ilold needs Rust (`cargo`) and Node.js (`node`, `npm`). One script builds and installs it:

```
git clone https://github.com/scab24/ilold-evm.git && cd ilold-evm && ./install.sh
```

`install.sh` installs `ilold` into `~/.cargo/bin`, replacing any earlier version, and stops with the missing tool's name when Rust or Node.js is absent. To update, run `git pull && ./install.sh`.

To build by hand, build the web app first: the Rust build embeds it and fails, naming the commands to run, when `crates/ilold-web/frontend/build/index.html` is missing.

```
npm --prefix crates/ilold-web/frontend ci
npm --prefix crates/ilold-web/frontend run build
cargo build --release
```

The binary is at `target/release/ilold` and serves the web app without the source checkout. `cargo build` does not replace an `ilold` already on your PATH; run `./target/release/ilold` to test a checkout. After changing the web app, build it again before building Rust.

## Running

Point ilold at a Foundry project — a directory containing a `foundry.toml`:

```
ilold explore tests/fixtures/staking
```

ilold compiles the project with solc, builds the model and CFGs, and drops you into the REPL:

```
  ╭──────────────────────────────────────────╮
  │ ilold explore — Staking                  │
  │ 8 functions | Type ? for help            │
  │ Web UI: http://127.0.0.1:52431           │
  ╰──────────────────────────────────────────╯

ilold[Staking]>
```

The port is assigned automatically unless you pass `--port`. The server listens on
127.0.0.1 unless you pass `--host <addr>`; `--host 0.0.0.0` exposes the API and the
embedded terminal to the network. Requests from other web pages and unknown host
names are refused ([Security](./reference/api-endpoints.md#security)).

Enter `browser` to open the web app. Closing the REPL stops its server.
`ilold explore --attach <url>` connects a REPL to a server that is already running,
such as one started with `ilold serve`, instead of starting a second one.
`--contract <name>` starts on a given contract; an unknown name exits with status 1
and suggests the closest names (`Did you mean: Vault?`).

Exit codes for every subcommand: `0` success, `1` error (unknown contract, not a
Foundry project, compile or server failure), `2` invalid command-line usage. `graph`
adds `3` and `4` ([Graph Export](./reference/graph-export.md)).

To read this guide locally, run `mdbook serve docs/guide --open`.

## First Session

A typical first exploration of a staking contract:

**1. Add a function call to the session:**

```
ilold[Staking]> c deposit

  + Step 0: deposit [public] external
    State writes:
      · balances
      · totalStaked
    Sequence: deposit
```

**2. Check accumulated state:**

```
ilold[→ deposit]> s

  ═══════════════════[ STATE ]═══════════════════
  balances
    balances[msg.sender] += msg.value  (deposit)
  totalStaked
    totalStaked += msg.value  (deposit)
```

**3. See who else touches a variable:**

```
ilold[→ deposit]> who totalStaked

  who: totalStaked
    Writers:
      [public] deposit
      [public] withdraw
    Readers:
      [public] getStakeInfo
  → sl deposit totalStaked, sl withdraw totalStaked
  → tl totalStaked
```

**4. Slice the data flow:**

```
ilold[→ deposit]> sl deposit totalStaked

  deposit · totalStaked — dataflow slice
  ════════════════════════════════════════════════════════════
  [backward]
    L42   require(msg.value > 0, "Zero deposit")
    L45   totalStaked += msg.value
  [forward]
    L47   emit Deposited(msg.sender, msg.value)
  → tr deposit | tl totalStaked
```

**5. Trace the full execution flow:**

```
ilold[→ deposit]> tr deposit

  ╭──────────────────────────────────────╮
  │ Staking::deposit()                   │
  │ modifiers: whenNotPaused             │
  │ max inlining depth: 2               │
  ╰──────────────────────────────────────╯

  001 │ ▶ deposit()
  002 │ ├─ ◇ require(!paused, "Paused")  [from: whenNotPaused]
  003 │ ├─ ◇ require(msg.value > 0, "Zero deposit")
  004 │ ├─ ✏ balances[msg.sender] += msg.value
  005 │ ├─ ✏ totalStaked += msg.value
  006 │ └─ ◆ emit Deposited(msg.sender, msg.value)
  → sl deposit balances, sl deposit totalStaked
```

## Inline Help

Type `?` at the prompt for the full command reference. Append `?` to any command for its usage:

```
ilold[Staking]> sl?
  slice <func> <var> [--backward]  Dataflow slice. Example: sl deposit totalStaked --backward
```
