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

`ilold serve <project>` exposes the graph as an interactive canvas (the home
view). Contracts are laid out left-to-right in the same topological order:

- **line color** is the relationship — `inherits`, `calls`, `holds`.
- **border style** is the kind — contract, abstract, interface, library.
- **border color** is the source subsystem (the top-level `src/` folder).
- **hover** a contract to highlight just its relationships and dim the rest.
- the **Relations / Kind** toggles filter the graph (hide call edges, dim
  interfaces, and so on) to cut through a dense protocol.

Click a contract to drop into its functions, where the per-function analysis
(`trace`, `slice`, `info`) takes over.

The header tabs switch the project page between **Map** and **Value flow**;
the tab and everything picked in it live in the URL, so a link
reopens the same view.

- **Value flow** draws who pays whom, left to right: who pays in, the project
  contracts, and what leaves the project. One arrow per pair and direction
  names its assets; Uniswap v4 `Currency` moves read "ETH or token, set by the
  pool" on amber dashed lanes. The asset buttons filter the arrows, a click on
  an arrow or a box lists its transfers (with their ids) in the side panel.

## Practical workflow

1. Run `analyze <project> --deps` to get the reading order.
2. Start at layer 0 — the interfaces and libraries everything else builds on.
3. For a contract you are about to audit, run `usedby <contract>` to learn its
   blast radius before touching it.
4. Run `deps <contract>` to see what it relies on, and open those first.
5. Use the canvas to spot tightly coupled clusters and cycles at a glance.
6. Drop into a contract and continue with the per-function workflow.

## Related pages

- [Contract Commands](../commands/contract.md) -- the `deps` and `usedby` commands
- [Full Audit Walkthrough](./audit-walkthrough.md)
- [HTTP API Reference](../reference/api-endpoints.md) -- the depgraph endpoints
- [Known Limitations](../reference/limitations.md) -- dependency graph caveats
