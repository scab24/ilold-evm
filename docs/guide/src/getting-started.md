# Getting Started

## Installation

Clone the repository and build from source:

```
git clone https://github.com/scab24/ilold.git
cd ilold
cargo build --release
```

The binary is at `target/release/ilold`.

## Running

Point ilold at a Foundry project — a directory containing a `foundry.toml`:

```
cargo run -- explore tests/fixtures/staking
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
127.0.0.1 unless you pass `--host <addr>`.

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
