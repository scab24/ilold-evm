# Mapping a Codebase

Before auditing a single function, it helps to understand how the whole project
fits together: which contracts are foundational, which build on top of them, and
what a change to one contract could affect. ilold builds a **contract dependency
graph** from three relationships — `inherits`, `calls` (resolved to the real
target contract), and `holds` (a state variable of another contract's type) —
and exposes it on the CLI, in the REPL, and as the web canvas.

The examples below use the `tests/fixtures/solc/cross` project, a small lending
example: an `IPool` interface, a `BasePool`, a `LendingPool is BasePool, IPool`,
a `Vault` that holds and calls an `IPool`, and a `SafeMath` library.

## Reading order

`analyze --deps` prints every contract grouped into topological layers. Layer 0
has no in-project dependencies (interfaces, libraries) and is what you read
first; each later layer builds on the earlier ones. Dependency cycles are
condensed so the order stays well defined.

```
ilold analyze tests/fixtures/solc/cross --deps

Dependency map · reading order (layer 0 = read first)

── layer 0 ──
  abstract  BasePool
  interface IPool
  library   SafeMath
── layer 1 ──
  contract  LendingPool   inherits→ BasePool, IPool
── layer 2 ──
  contract  Vault         calls→ IPool, SafeMath  ·  holds→ IPool  ·  dispatches→ LendingPool

legend: inherits→  calls→  holds→  dispatches→
```

This tells you to read `IPool`, `BasePool` and `SafeMath` first, then
`LendingPool`, then `Vault`. `dispatches→` names the deployable contracts that
may answer the calls a contract makes through an interface: `Vault` calls
`IPool`, which `LendingPool` implements, so `Vault` is read after it. This is
the same graph the web canvas and the REPL `deps`/`usedby` show.

## Following one contract

Inside the REPL, `deps` shows what a contract depends on:

```
ilold[Vault]> deps

  Vault → depends on
    IPool     calls×2 holds
    SafeMath  calls×2
```

`usedby` is the reverse — the blast radius. Before you change or sign off on a
contract, this is who could be affected:

```
ilold[Vault]> usedby IPool

  IPool ← used by (blast radius)
    LendingPool  inherits
    Vault        calls×2 holds
```

`IPool` is inherited by `LendingPool` and used by `Vault`, so a change to the
interface ripples to both. The same focused view is available without the REPL
via `analyze --deps --contract IPool`.

## The visual graph

`ilold serve <project>` opens on the project page. Its **Map** tab draws the
contracts by source folder, with interfaces, libraries and the interfaces
outside the project folded into one box each (click a box to show its
contracts, **Fold** to fold it again):

- one line per pair of contracts adds up the calls between them, function to
  function (the [`links --all`](../commands/contract.md#links) pairs); the chip
  counts them, `4 down · 1 up` or `3 → · 1 ←` when they go both ways.
- the **Lines** legend at the bottom is the only filter: Calls, Reads, Value,
  Library, Inherits, Holds, Via interface and Unresolved each cycle between
  shown, dimmed and hidden (Unresolved is never hidden). Value lines are green,
  AMBIGUOUS and unresolved ones amber; an abstract contract has a dashed border.
- **hover** a contract to fade every box it has no drawn line with and animate
  its own lines. **Hover** a line for its counts and call sites, each with how
  the call resolved (via interface, selector match or this call), the state
  variable holding the address and the AMBIGUOUS tag; click it to list its
  calls in the side panel.
- **click** a contract to focus on it: Callers / Both / Callees and Depth 1 or 2
  choose the neighbourhood, the panel lists its calls, callers and unresolved
  calls with links to the source. Click it again, or press Enter, to open its
  functions (Enter on a focused button or link presses that control instead);
  Escape clears the focus. Find a contract selects the best match of its name
  among the project contracts, then the project libraries and interfaces, then
  dependencies and targets outside the project (deployable contracts first among equal matches), and zooms
  to it; it says so
  when nothing matches. Each deployable contract box shows its entry points,
  restricted functions and whether it moves value.
- with nothing selected the panel shows the project totals, each call counted
  once (the `in all` lines of `links --all`), and the five most connected
  contracts.

The header tabs switch the project page between **Map**, **Value flow** and
**Route**; the tab and everything picked in it live in the URL, so a link
reopens the same view. The reading order by layers stays in
`analyze --deps`.

- **Value flow** draws who pays whom, left to right: who pays in, the project
  contracts, and what leaves the project. One arrow per pair and direction
  names its assets; Uniswap v4 `Currency` moves read "ETH or token, set by the
  pool" on amber dashed lanes. The asset buttons filter the arrows, a click on
  an arrow or a box lists its transfers (with their ids) in the side panel and
  zooms to the parties it touches.
- **Route** shows how one deployed contract reaches another (the `route`
  command): pick From and To, Swap, and Include reads to also follow read
  calls. Each hop is drawn function to function; a hop inside a contract that
  the call graph does not record is marked "steps inside not recorded". When
  several routes exist, the boxes list the functions of the chosen route and
  fold the others into one "more on other routes" row.

## Practical workflow

1. Run `analyze <project> --deps` to get the reading order.
2. Start at layer 0 — the interfaces and libraries everything else builds on.
3. For a contract you are about to audit, run `usedby <contract>` to learn its
   blast radius before touching it.
4. Run `deps <contract>` to see what it relies on, and open those first.
5. Use the Map to spot tightly coupled folders and the most connected contracts.
6. Drop into a contract and continue with the per-function workflow.

## Related pages

- [Contract Commands](../commands/contract.md) -- the `deps` and `usedby` commands
- [Full Audit Walkthrough](./audit-walkthrough.md)
- [HTTP API Reference](../reference/api-endpoints.md) -- the depgraph endpoints
- [Known Limitations](../reference/limitations.md) -- dependency graph caveats
