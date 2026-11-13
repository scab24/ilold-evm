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
contracts, **Fold** to fold it again). The first view shows the whole map
when it is readable at that size, otherwise it fits the project folders and
any group opened in the URL; drag the canvas to reach the rest:

- one line per pair of contracts adds up the calls between them, function to
  function (the [`links --all`](../commands/contract.md#links) pairs); the chip
  counts them, `4 down · 1 up` or `3 → · 1 ←` when they go both ways.
- the **Lines** legend at the bottom is the only filter: Calls, Reads, Value,
  Library, Inherits, Holds, Via interface and Unresolved each cycle between
  shown, dimmed and hidden (Unresolved is never hidden). A line's colour names
  the relation (calls light grey, reads cyan, library grey, value green,
  inherits purple, holds orange, unresolved amber) and its dash how the target
  was found ("Found by": through the interface solid, selector match dashed,
  `this` call dotted); AMBIGUOUS calls are amber. The workbench link rows and
  legend use the same colours and dashes. An abstract contract has a dashed
  border.
- **hover** a contract to fade every box it has no drawn line with and animate
  its own lines. **Hover** a line for its counts and call sites, each with how
  the call resolved (via interface, selector match or this call), the state
  variable holding the address and the AMBIGUOUS tag; click it to list its
  calls in the side panel.
- **click** a contract to focus on it: Callers / Both / Callees and Depth 1 or 2
  choose the neighbourhood, the panel lists its calls and reads, callers,
  library calls and unresolved calls with links to the source. Each list
  counts call sites, the unit of the totals above it, and says "N of M shown"
  when the legend hides some. Click it again, or press Enter, to open its
  functions (Enter on a focused button or link presses that control instead);
  Escape clears the focus. Find a contract selects the best match of its name
  among the project contracts, then the project libraries and interfaces, then
  dependencies and targets outside the project (deployable contracts first among equal matches), and zooms
  to it; it says so
  when nothing matches. Each deployable contract box shows its entry points,
  restricted functions and whether it moves value.
- with nothing selected the panel shows the project totals, each call counted
  once (the `in all` lines of `links --all`), and the five most connected
  contracts. "Value transfers on these calls" counts the moves made by calls a
  project contract or library answers.
- two dependency libraries with the same name (two `SafeCast`) show their
  package (`library from v4-core`) under the name, in the panel and in the
  call rows.

The header (ilold, "execution path analyzer", the contract count, the tabs
and ⌘K Search) is the same on the project page and in the contract
workbench. The header tabs switch the project page between **Map**, **Value flow** and
**Route**; the tab and everything picked in it live in the URL, so a link
reopens the same view, and the browser Back and Forward buttons step through
the contracts, pairs, arrows and route ends you picked (filters change the URL
without adding steps). On every tab Escape clears the selection and Enter opens
the selected contract; tooltips stay inside the canvas. The reading order by
layers stays in `analyze --deps`.

- **Value flow** draws who pays whom, left to right: who pays in, the project
  contracts, and what leaves the project. One arrow per pair and direction
  names its assets; Uniswap v4 `Currency` moves read "ETH or token, set by the
  pool" on amber lanes. The asset buttons filter the arrows, a click on
  an arrow or a box lists its transfers in the side panel and zooms to the
  parties it touches; the "Users and addresses" box names the first three
  addresses and counts the rest as `+N`. The panel counts every recognised
  move, so it also includes the transfers made on calls to targets outside
  the project, which the Map totals leave out.
- **Route** shows how one deployed contract reaches another (the `route`
  command): pick From and To, Swap, and Include reads to also follow read
  calls. Each hop is drawn function to function; a hop inside a contract that
  the call graph does not record is marked "steps inside not recorded". When
  several routes exist, the boxes list the functions of the chosen route and
  fold the others into one "more on other routes" row. Hover a hop for its
  calls; click a box to select it (a box of another route switches to that
  route), click it again or press Enter to open the contract.

## The contract workbench

Opening a contract keeps the project header: its tabs lead back to the Map,
Value flow and Route, and after the contract name the mode control switches
between CFG, Seq, Session and Connections. The mode lives in the URL
(`?mode=cfg`, `?mode=seq`, `?mode=session`; Connections when absent), so a
reload or a link keeps it and the browser Back button returns to the previous
mode. The bar under the header holds the tools of the current mode: the
Connections trail and controls, the Seq layout (down or right), Back and Clear
of the active scenario (Session only), Center and Terminal. A highlight sent
from the REPL or an MCP client (`ilold_function_analysis`, `ilold_trace`)
switches to Seq and shows the function there.

**Connections** is a board that accumulates what you open, function to
function (the [`links`](../commands/contract.md#links) rows):

- the contract opens with only its functions that call or are called by other
  contracts (**All functions** lists the rest); the contracts it calls are
  folded on its right, its callers on its left, each band labelled
  ("LaunchCurve calls", "Calls LaunchCurve"); libraries and the interfaces
  outside the project fold into one box each.
- click a function (on the board or in the left list) to select it: its lines
  light up, the others dim. Click a function in a folded box to open that
  contract in place, one column further out, with that function selected.
- the trail above the board lists the opened steps (each one selectable; a
  long trail keeps the first and last step and folds the middle into `+N`).
  **Backspace** removes the last step, **×** closes a box (and the boxes
  opened further out on its side), **Reset** returns to the start. Only the
  two boxes used last stay open; older ones fold into strips at the left edge
  (click a strip to open it again). When the board is wider than the canvas,
  the `← N` / `N →` buttons scroll to the boxes out of view.
- **Callers / Both / Callees** keep one side, the **Lines** legend shows,
  dims or hides each kind of line as on the Map (Unresolved is never hidden).
- double-click a box (or press Enter on its title) to make it the main
  contract of the board. Delete removes nothing in this mode.
- the whole board (open boxes, trail, selection, direction, filters) lives in
  the URL, so a reload or a link reopens it; it is not saved in scenarios.

The function list on the left selects in every mode (in Connections it selects
on the board). At its foot, **Connections · N → · M ←** counts the calls and
reads this contract makes to other contracts and receives from them
(libraries and the contract itself not counted); clicking it switches to
Connections with the contract summary in the Inspector. The Inspector lists
the selected function's calls, callers, reads, readers, library calls, value
moves and unresolved calls, one row each with a link to the source line (and
the assignment of the address it calls through), and **Open X** for a
function of another contract. Below it the contract summary shows the
**Phases** (the state variables that gate which entry points can run; click
a function to select it on the board, or to add it to the canvas in CFG and
Seq) and the contract's **Value flows**.

The legend bar at the bottom of the workbench is the same as on the project
page: in Connections it filters the lines, in the other modes it explains the
node colours. The status bar shows the server connection, how many nodes are
selected and the keys of the current mode (H pan, V select, Delete remove in
CFG and Seq; Backspace undo and Esc clear in Connections). The embedded
terminal starts closed: **Terminal** opens it above the legend, its red
button closes it. Below 1200px wide the function list starts folded; its
arrow opens it. Panels, section titles, chips and buttons use
the same styles as the project page.

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
