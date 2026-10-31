# Known Limitations

This page documents the current analysis boundaries of ilold. Understanding these limitations is necessary for interpreting slice, timeline, and trace results correctly.

## Intraprocedural slicing only

The dataflow slicer operates within a single function body (plus its inlined modifiers). It does not follow values across function call boundaries. If `deposit` calls an internal helper `_updateBalance(amount)`, the slice for `amount` in `deposit` will show the call site but not the writes inside `_updateBalance`. Use `tr <func>` to inspect internal call bodies separately.

## Assignment-only DEF extraction

Only `Assignment` expressions (`x = ...`, `x += ...`, `x -= ...`) produce DEF entries in the slicer's use-def analysis. Solidity mutations that are not modeled as assignments -- specifically `x++`, `x--`, `++x`, `--x`, `delete x`, `arr.push(v)`, and `arr.pop()` -- are captured as USEs of the target variable but not as DEFs. A backward slice on a variable mutated exclusively through `.push()` will miss the mutating statement as a definition point.

## Modifier placeholder split

Modifier bodies are split at the first top-level `_;` (placeholder) statement to separate "before" code from "after" code. If the placeholder appears inside a nested block (e.g., inside an `if` branch), the entire modifier body is treated as "before" code. This is over-inclusive: statements that should execute after the function body will appear before it in the flattened view. In practice, most modifiers place `_;` at the top level, so this rarely triggers.

## Forward slice over-tainting via ancestor merge

When a statement is included in a forward slice, its lexical ancestors (enclosing `if`, `for`, `while` blocks) are also included so that the rendered slice shows control-flow context. The ancestor's condition variables are merged into the tainted set. This means an `if (unrelatedCondition)` enclosing a tainted write will add `unrelatedCondition` to the taint set, potentially pulling in unrelated statements in subsequent iterations. The result is a conservative (larger) slice rather than a precise one.

## Tuple destructuring

Tuple destructuring assignments such as `(a, b) = foo()` may not be recognized as DEFs depending on how the Solidity frontend lowers them. If the frontend does not emit a top-level `Assignment` node, the individual targets (`a`, `b`) are treated as USEs only. This can cause a backward slice to miss the destructuring as a definition point for `a` or `b`.

## Effects across calls

State reads and writes are resolved through compiler declarations, so a local
that shadows a state variable is never reported as state. Effects of callees --
internal functions, internal library functions and library delegatecalls --
are reported as transitive effects with the call chain (`via`) that reaches
them, including helpers called from modifiers. A library parameter declared
`storage` is bound at each call site to the caller's argument, so
`reserve.liquidityIndex = x` inside a library becomes a write of
`_reserves[].liquidityIndex` in the calling contract. Inherited functions are
analysed as the derived contract runs them, overrides included.

Boundaries:

- Path trees stay per function: `revert_paths` counts only the function's own
  code. `can_revert` and the revert sources include reverts in callees.
- A storage pointer follows the last assignment in source order, not each
  branch separately. A function returning a storage pointer is followed one
  level deep, and only when all its returns agree on the location.
- Recursive calls contribute their effects once; chains deeper than 24 calls
  are cut.
- Writes inside inline assembly and calls through `address.call` or `this.f()`
  into the same contract are not followed.

## Timeline tracks state mutations only

The `timeline` command tracks writes to state variables across session steps. Local variable assignments within a function body are recorded separately (`local_entries`) but are not visible in the default timeline output. If you need to trace a local variable, use `slice` within the specific function instead.

## Session requires at least one call

The `timeline`, `state`, and `sequence` endpoints require an active session with at least one `Call` step. The `timeline` and `state` commands return empty results if no steps have been added. The `sequence` command requires at least two steps. Use `tr <func>` for read-only inspection of a function's flow without adding it to the session.

## Internal and private functions cannot be session entry points

Session steps model real external transactions. Functions with `internal` or `private` visibility cannot be called from outside the contract, so they cannot be added as session steps via `c <func>`. Use `tr <func>` to inspect their execution flow, or call a public/external function that invokes them to see their effects through the modifier and internal-call inlining in the trace.

## Project must compile

The solc frontend compiles the whole Foundry project to produce the AST it
analyzes. If the project does not compile -- missing dependencies, a version
mismatch, an uninitialized submodule -- there is no AST and analysis cannot run.
ilold reports the compiler error rather than analyzing a partial model. Point it
at a directory with a `foundry.toml` whose contracts build.

## Dependency graph keys contracts by name

The contract dependency graph (`deps`, `usedby`, `analyze --deps`, the canvas)
identifies contracts by name. If a project declares the same name twice -- for
example two `EIP712Hash` libraries in different folders -- they collapse into a
single node, and one's relationships can mask the other's. Cross-contract call
edges also require a typed target: calls through `address.call(...)` or assembly
have no resolvable contract and produce no `calls` edge.

## Related pages

- [Taint Analysis](../workflows/taint-analysis.md) -- forward slice caveats in practice
- [Contract Commands](../commands/contract.md) -- the `deps` and `usedby` commands
- [HTTP API Reference](./api-endpoints.md)
