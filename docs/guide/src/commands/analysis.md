# Analysis Commands

Analysis commands query the contract model without modifying the session. Each command produces cross-reference hints at the bottom of its output, suggesting related commands to run next.

## who

`w <variable>` or `who <variable>`

Shows which functions read and write a state variable, with their access level. Searches across the active contract and its ancestors.

```
ilold[Staking]> who totalStaked

  who: totalStaked
    Writers:
      [public] deposit
      [public] withdraw
    Readers:
      [public] getStakeInfo
  → sl deposit totalStaked, sl withdraw totalStaked
  → tl totalStaked
```

The cross-reference hints suggest running [slice](#slice) for each writer and [timeline](#timeline) for the variable.

## info

`i <function>` or `info <function>`

Displays a full function narrative: execution paths, state reads/writes, internal and external calls, transitive effects through the call chain, and observations. Works on any function including internal ones.

```
ilold[Staking]> i withdraw

  withdraw [Public] — whenNotPaused, nonReentrant
  ├── Paths: 3 total, 1 happy, 2 revert
  ├── Guards:
  │     pause: require(!paused, "Paused") (via modifier whenNotPaused)
  │     reentrancy: require(!locked) (via modifier nonReentrant)
  ├── State reads: balances, totalStaked
  ├── State writes: balances, totalStaked
  ├── External calls: msg.sender.call{value: amount}
  ├── Transitive effects:
  │     via _updateRewards:
  │       writes: rewardDebt
  │       reads: rewardPerToken
  └── Observations:
        · External call after state writes (checks-effects-interactions followed)
  → c withdraw, tr withdraw
```

Guards list each revert check that gates the function, with the chain that reaches it (`modifier m`, then each helper as `Contract::function`). The observation "Public function writes state without access control" appears only when no access guard (`caller`, `role`, `tx.origin`) was found; pause, reentrancy and initializer guards do not count.

The `info` command does not require the function to be in the session. It analyzes the function in isolation.

## trace

`tr <function> [--depth N] [--reverts] [+N...] [-i]`
`tr step <N>`

Renders the execution flow tree for a function. Modifier bodies are inlined into the tree with `[from: modifier]` annotations. Internal calls are expanded up to the depth limit (default 2).

```
ilold[Staking]> tr withdraw

  ╭──────────────────────────────────────╮
  │ Staking::withdraw(uint256)           │
  │ modifiers: whenNotPaused, nonReentrant│
  │ max inlining depth: 2               │
  ╰──────────────────────────────────────╯

  001 │ ▶ withdraw(uint256)
  002 │ ├─ ◇ require(!paused, "Paused")  [from: whenNotPaused]
  003 │ ├─ ◇ require(!locked)  [from: nonReentrant]
  004 │ ├─ ✏ locked = true  [from: nonReentrant]
  005 │ ├─ ◇ require(amount <= balances[msg.sender])
  006 │ ├─ ○ _updateRewards(msg.sender)  [+8 ops, depth limited]
  007 │ ├─ ✏ balances[msg.sender] -= amount
  008 │ ├─ ✏ totalStaked -= amount
  009 │ ├─ → msg.sender.call{value: amount}
  010 │ ├─ ◆ emit Withdrawn(msg.sender, amount)
  011 │ └─ ✏ locked = false  [from: nonReentrant]

  tip: expand with `tr <func> +N` — candidates: 6
  → sl withdraw balances, sl withdraw totalStaked
```

### Icon legend

| Icon | Meaning |
|------|---------|
| `▶` | Function entry |
| `◇` | require/assert |
| `✏` | State write |
| `▸` | State read |
| `○` | Internal call |
| `→` | External call |
| `◆` | Event emission |
| `?` | Branch (if/else) |
| `↻` | Loop header |
| `✓` | Return |
| `✗` | Revert |

### Options

- `--depth N` -- Set max inlining depth for internal calls. Default is 2.
- `--reverts` -- Include revert paths in the tree.
- `+N` -- Force-expand a depth-limited internal call at step N. Multiple `+N` flags allowed.
- `-i` -- Open the trace in an interactive TUI with keyboard navigation. Increases default depth to 4.
- `step N` -- Re-render the persisted flow tree from session step N. `-i` opens it in the TUI;
  `--depth`, `--reverts` or `+N` after `step N` rebuild that step's function with those flags.

Unknown flags, a non-numeric `--depth`, `trace step` without an index, a step that does not
exist and extra words are rejected with a message instead of being ignored.

```
ilold[Staking]> tr withdraw +6

  (same tree with _updateRewards fully expanded at step 6)
```

```
ilold[Staking]> tr step 0

  (renders the persisted trace from session step 0)
```

### Cross-contract calls

External calls (`→`) to a typed contract are resolved to the real target. When
`Vault` calls `pool.supply(...)` through an `IPool pool` variable, or casts an
address with `IPool(addr).supply(...)`, the solc frontend resolves both to
`IPool.supply` rather than an unnamed placeholder. The resolved target is what
`info`, `trace`, the call graph, and `deps`/`usedby` report. Low-level calls
(`addr.call{value: v}(...)`, `delegatecall`, `staticcall`) and value transfers
(`transfer`, `send`) have no typed target: they are reported by kind against the
address expression, with their call options. Inline assembly is not resolved.
Internal library calls (`x.f()` through `using for`, or `L.f()` for an internal
`f`) are internal calls written `L.f`; public and external library functions are
delegatecalls and count as external.

## timeline

`tl <variable>` or `timeline <variable>`

Shows the cross-step mutation history of a variable across the current session. Each mutation includes the operator, value expression, and path conditions (reached-when).

```
ilold[→ deposit → withdraw]> tl totalStaked

  totalStaked — mutation timeline
  ════════════════════════════════════════════════════════════
  [state]
    session step 0 deposit
      ✏ totalStaked += msg.value [trace step 5]
    session step 1 withdraw
      ✏ totalStaked -= amount [trace step 8]
        reached when:
          · amount <= balances[msg.sender]
  → sl deposit totalStaked, sl withdraw totalStaked
```

If the variable has no mutations in the current session, timeline tells you to add steps with `c <func>` first.

## slice

`sl <function> <variable> [--backward|--forward|--both]`

Performs dataflow analysis on a variable within a function. Walks the function body and modifier bodies to find definitions (backward) and uses (forward) of the variable. Modifier entries are prefixed with `[mod name]`.

The direction flags can appear in any position: `sl --backward deposit totalStaked` and `sl deposit totalStaked --backward` are equivalent. The default direction is `--both`.

```
ilold[Staking]> sl withdraw balances --backward

  withdraw · balances — dataflow slice
  ════════════════════════════════════════════════════════════
  [backward]
    L31   require(amount <= balances[msg.sender])
    L34   balances[msg.sender] -= amount
  → tr withdraw | tl balances
```

```
ilold[Staking]> sl withdraw totalStaked --both

  withdraw · totalStaked — dataflow slice
  ════════════════════════════════════════════════════════════
  [backward]
    L35   totalStaked -= amount
  [forward]
    L38   emit Withdrawn(msg.sender, amount)
  → tr withdraw | tl totalStaked
```

Short flags are also accepted: `-b` for `--backward`, `-f` for `--forward`.

When a variable is defined inside a modifier body, the entry shows its origin:

```
  [backward]
    L12   [mod whenNotPaused] require(!paused, "Paused")
    L35   totalStaked -= amount
```
