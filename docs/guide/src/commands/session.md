# Session Commands

Session commands manage the call sequence -- the ordered list of function calls that represents an execution scenario you want to analyze.

## call

`c <function>` or `call <function>`

Adds a function call to the session sequence. Only external and public functions are accepted; internal and private functions are rejected since they cannot be entry points for a real transaction.

```
ilold[Staking]> c deposit

  + Step 0: deposit [public] external
    State writes:
      · balances
      · totalStaked
    Sequence: deposit
```

```
ilold[→ deposit]> c withdraw

  + Step 1: withdraw [public] external
    State writes:
      · balances
      · totalStaked
    Sequence: deposit → withdraw
```

Attempting to call an internal function:

```
ilold[Staking]> c _updateRewards

  '_updateRewards' is internal and cannot be called from outside the contract —
  not a valid session entry point. Use `tr _updateRewards` to view its flow,
  or `c <public_caller>` to trace a real entry point.
```

## back

`b` or `back`

Removes the last step from the session sequence.

```
ilold[→ deposit → withdraw]> b

  - Step removed. 1 remaining.
    Sequence: deposit
```

## clear

`cl` or `clear`

Resets the session, removing all steps. Prompts for confirmation if steps exist.

```
ilold[→ deposit → withdraw]> cl
  Clear 2 steps? (y/n)
  y
  Session cleared.
```

## state

`s` or `state`

Shows the accumulated state mutations across all steps in the session. Each variable lists every mutation with the operator symbol (`+=`, `-=`, `=`) and the originating function.

```
ilold[→ deposit → withdraw]> s

  ═══════════════════[ STATE ]═══════════════════
  balances
    balances[msg.sender] += msg.value  (deposit)
    balances[msg.sender] -= amount  (withdraw)
  totalStaked
    totalStaked += msg.value  (deposit)
    totalStaked -= amount  (withdraw)
```

If the session is empty, `state` tells you to add steps first.

## sequence

`seq` or `sequence`

Displays a narrative of the current call sequence, including dependencies between steps and observations about the interaction pattern. Requires at least 2 steps.

```
ilold[→ deposit → withdraw]> seq

  Step 0: deposit
    writes: balances, totalStaked

  Step 1: withdraw
    writes: balances, totalStaked
    depends on: deposit (shared state: balances, totalStaked)

  Observations:
    · deposit and withdraw modify the same variables (balances, totalStaked)
```

## step

`st <index>` or `step <index>`

Re-inspects a specific session step, showing its full function narrative (same output as [info](./analysis.md#info)). You can also write `st0`, `st1` without a space. The index is 0-based; a non-numeric or out-of-range index says how many steps the scenario has.

```
ilold[→ deposit → withdraw]> st 0

  deposit [public] — whenNotPaused
  ├── Paths: 2 total, 1 happy, 1 revert
  ├── State reads: balances
  ├── State writes: balances, totalStaked
  └── Events: Deposited
```

## session

`ss` or `session`

Shows the full session overview: active contract, current step sequence, and findings count.

```
ilold[→ deposit → withdraw]> ss

  Contract: Staking
  Steps:    deposit → withdraw
  Findings: 0
```
