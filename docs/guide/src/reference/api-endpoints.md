# HTTP API Reference

ilold exposes an HTTP API on the configured port (default 3001). All endpoints return JSON. The API is split into three groups: command bus, session queries, and contract queries.

## Command bus

### POST /api/cmd

Execute a session command. This is the single entry point for all state-mutating session operations.

Each contract owns an independent session store (scenarios, forks and steps). A command only touches the store of the contract it names, and that contract becomes the server's **current contract**. Naming another contract never discards the previous contract's session. At startup the current contract is the project default: the most derived deployable contract (abstract contracts, libraries and interfaces only as fallbacks), ties broken by name. `explore` uses the same rule.

**Request body:**

```
{
  "contract": "Staking",          // optional, defaults to the current contract
  "command": { "Call": { "func": "deposit" } }
}
```

**Supported commands:**

| Command | Payload | Description |
|---|---|---|
| `Call` | `{ "func": "deposit", "trace_config": null }` | Add a function call as a session step |
| `Back` | `"Back"` | Remove the last step |
| `Clear` | `"Clear"` | Remove all steps |
| `State` | `"State"` | Return accumulated state variable summary |
| `Functions` | `"Functions"` | List functions of the current contract |
| `FunctionsAll` | `"FunctionsAll"` | List all accessible functions including inherited |
| `StateVarsAll` | `"StateVarsAll"` | List all accessible state variables including inherited |
| `Who` | `{ "variable": "totalStaked" }` | Find all writers and readers of a variable |
| `Finding` | `{ "severity": "High", "title": "...", "description": "..." }` | Record an audit finding |
| `Note` | `{ "text": "..." }` | Record a free-text note |
| `Status` | `{ "func": "deposit", "status": "Reviewed" }` | Set review status for a function |
| `Session` | `"Session"` | Return session overview (contract, steps, finding count) |
| `Export` | `"Export"` | Export session journal as markdown |
| `SaveSession` | `"SaveSession"` | Serialize session to JSON |
| `LoadSession` | `{ "json": "..." }` | Restore session from serialized JSON |

**Response:** A `CommandResult` variant matching the command type. Key variants:

- `StepAdded`: `step_index`, `function`, `access`, `state_changed` (list of variable names)
- `StateView`: `summary` (list of variable summaries with writers per step)
- `FunctionList`: `functions` (name, access level, writes_state, has_external_calls)
- `VariableInfo`: `variable`, `writers` (name + access), `readers` (name + access)
- `Error`: `message`

## Session query endpoints

These endpoints read the active scenario of one contract's session. They never change which contract is current.

Every endpoint in this section, plus `GET /api/scenarios` and `GET /api/scenarios/all`, accepts an optional `?contract=<name>` query parameter:

- With `contract`, the endpoint reads that contract's session. An unknown contract returns 404. A contract with no commands yet reads as an empty `main` scenario.
- Without `contract`, the endpoint reads the current contract: the one named by the most recent `POST /api/cmd`, or the project default before any command. This keeps older clients working, but clients that work on a specific contract should pass it.

`GET /api/scenarios/all` includes the resolved `contract` in its response.

### GET /api/session/state

Returns the accumulated state variable summary across all session steps.

**Response:** Array of `VariableSummary` objects with variable name, type, and per-step write details.

### GET /api/session/sequence

Returns a sequence narrative describing the relationship between session steps. Requires at least 2 steps.

**Response:** `SequenceNarrative` with per-step summaries and flow summaries derived from persisted flow trees.

### GET /api/session/step/{index}/narrative

Returns the function narrative for a specific session step.

**Path params:** `index` -- zero-based step index.

**Response:** `FunctionNarrative` with paths, state writes, external calls, and conditions.

### GET /api/session/step/{index}/trace

Returns the persisted FlowTree of a session step. This is the tree captured when `Call` was executed, not a recomputation.

**Path params:** `index` -- zero-based step index.

**Response:** `FlowTree` with step nodes, each containing operation type, target, conditions, and child steps. Returns 404 if the step has no persisted tree (pre-Phase-2a sessions).

### GET /api/session/timeline/{variable}

Returns a chronological timeline of every write to `variable` across all session steps. Matches by base name (e.g., `balances` matches `balances[msg.sender]`).

**Path params:** `variable` -- state variable name.

**Response:** `VariableTimeline` with `state_entries` and `local_entries`. Each entry contains: `session_step_index`, `function`, `target`, `operator`, `value_expr`, `reached_when` (path conditions), `via` (modifier name if applicable), `scope`.

### GET /api/session/slice/{function}/{variable}

Returns a dataflow slice for `variable` inside `function` of the session's current contract.

**Path params:** `function` -- function name, `variable` -- variable name.

**Query params:**

| Param | Values | Default | Description |
|---|---|---|---|
| `direction` | `backward`, `forward`, `both`, `b`, `f`, `all` | `both` | Slice direction |

**Response:** `SliceResult` with `backward` and `forward` arrays. Each entry contains: `path`, `span` (source location), `text` (rendered statement), `origin` (`FunctionBody` or `Modifier(name)`).

### GET /api/session/trace/{contract}/{func}

Returns a FlowTree for the given function, computed on demand from the current analysis data.

**Path params:** `contract` -- contract name, `func` -- function name.

**Query params:**

| Param | Type | Default | Description |
|---|---|---|---|
| `depth` | integer | 2 | Maximum inline depth for internal calls |
| `reverts` | boolean | false | Include revert paths in the tree |
| `expand` | string | empty | Comma-separated step IDs to force-inline beyond max depth (e.g., `17,24`) |

**Response:** `FlowTree` with nested step nodes representing the execution flow.

### GET /api/session/function/{contract}/{func}

Returns a function narrative without requiring a session step. Useful for inspecting functions that have not been called in the session.

**Path params:** `contract` -- contract name, `func` -- function name.

**Response:** `FunctionNarrative` with paths, state effects, and behavioral summary.

## Contract query endpoints

These endpoints read from the static analysis data and do not require an active session.

### GET /api/project

Returns a project summary with file count and a list of contracts (name, kind, function count, state variable count, inheritance).

### GET /api/project/map

Returns the full project map with all contracts, their functions (with path counts and external call flags), state variables, and cross-contract relationships extracted from call graphs.

### GET /api/project/depgraph

Returns the contract dependency graph in Cytoscape-compatible JSON. Nodes carry the contract name, kind, source folder, and topological layer. Edges carry the relationship kinds (`inherits`, `calls`, `holds`) and call count.

### GET /api/contract/{name}

Returns contract detail: name, kind, inheritance chain, functions (with path stats), state variables, and inherited functions and state variables. Each function carries its `signature` and the `key` to use as `{func}` in the routes below: the bare name when it is unique on this contract, otherwise the signature.

Signatures are `name(type,...)`:

- External and public functions use the ABI form, without data locations: `sums(uint256[])`.
- Internal and private functions keep each parameter's location, because overloads may differ only by it: `_sum(uint256[] calldata)`, `_sum(uint256[] storage)`.
- `struct`, `enum` and `contract` prefixes are dropped (`DataTypes.ReserveData`, `IPool`). Function-type parameters are written `function(uint256) external returns (bool)`.
- The receive and fallback functions have the bare signatures `receive` and `fallback`. A regular function with one of those names (solc warns about it) is always keyed by its signature, e.g. `fallback()`.

A bare-name key is not stable: adding an overload later turns an existing key such as `put` into `put(uint256)`. Clients that store function references should prefer the signature.

### GET /api/contract/{name}/callgraph

Returns the call graph for a contract in Cytoscape-compatible JSON format, built as the most derived contract: its own and inherited functions, with `super` following the C3 linearization and calls to virtual functions dispatched to the visible override. Node ids are `Contract::name`, or `Contract::name(types)` for overloaded functions; `is_external` marks nodes outside the contract's hierarchy. Edges carry the call `kind`, a `kind_label`, the call count and, for dispatched calls, the `static_target` named in the source. Kinds:

| Kind | Meaning |
|------|---------|
| `Internal` | JUMP within the hierarchy, including `super.f()` and `Base.f()` |
| `InternalLibrary` | JUMP into an internal library function, directly or through `using for` |
| `LibraryDelegatecall` | DELEGATECALL into a public or external library function |
| `External` | Call through an interface or contract type, including casts such as `IFoo(addr).f()` |
| `ExternalSelf` | `this.f()` |
| `LowLevelCall`, `LowLevelDelegatecall`, `LowLevelStaticcall` | `address.call/delegatecall/staticcall`, node `address::call` etc. |
| `Transfer`, `Send` | `payable(to).transfer(v)` / `.send(v)` |
| `Creation` | `new C(...)`, also with `{salt: s}`; node `C::constructor` |

Compiler builtins (`keccak256`, `abi.*`, `ecrecover`, `type(...)`, `require`, ...) are not calls.

### GET /api/contract/{name}/depgraph

Returns the contract's neighbourhood in the dependency graph: the contract itself (flagged `focus`), the contracts it depends on, and the contracts that depend on it. Same node and edge shape as `/api/project/depgraph`.

### Function routes and overloads

`{func}` in `/api/contract/{name}/{func}/cfg`, `/paths` and `/source` is a bare function name when it is unambiguous on the contract (own and inherited public functions), or a full signature such as `setPoolPause(bool,uint40)`. Whitespace in a signature is ignored, so `_sum(uint256[]storage)` matches `_sum(uint256[] storage)`. A bare name shared by several overloads returns `409 Conflict` with `{"error": ..., "candidates": ["setPoolPause(bool,uint40)", "setPoolPause(bool)"]}`. All three routes select the same overload for the same `{func}`.

### GET /api/contract/{name}/{func}/cfg

Returns the control flow graph for a function in Cytoscape-compatible JSON format. Nodes represent basic blocks (entry, normal, return, revert, assembly, loop). Edges represent control flow transitions.

### GET /api/contract/{name}/{func}/paths

Returns the path tree for a function. Contains all execution paths with stats (total, happy, revert) and per-path annotations (state writes, external calls, events). `internal_calls` lists JUMPs (`f`, `super.f`, `Base.f`, `Library.f` for internal library functions), and `internal_call_targets` repeats them in the same order with the `declaration` solc resolved (`[contract, key]`). `external_calls` lists everything that leaves the frame, each with its call `kind`.

### GET /api/contract/{name}/sequences

Returns the sequence tree for a contract, showing function interaction patterns.

### GET /api/contract/{name}/analysis

Returns the sequence analysis for a contract: per-function behavior summaries (state writes, state reads, external calls, conditions) and inter-function transition information.

### GET /api/contract/{name}/suggestions

Returns search suggestions for the contract: function names, state variable names, event names, external call targets, and predefined categories (revert, return, assembly).

## WebSocket

### GET /ws

Upgrades to a WebSocket connection. The server pushes JSON messages when session state changes. Every session message carries the `contract` whose session changed, so clients ignore messages for other contracts:

- `session_add_node`: a step was added (scenario, function, access level, step index)
- `session_remove_node`: the last step was removed
- `session_clear`: all steps were cleared
- `scenario_created`, `scenario_switched`, `scenario_deleted`, `scenario_forked`, `scenario_store_reloaded`: scenario lifecycle events
- `session_resync` (no `contract`): the client fell behind and messages were dropped; refetch the session with `GET /api/scenarios/all?contract=<name>`

### GET /ws/pty

Upgrades to a WebSocket bridged to an `ilold explore --attach` REPL, which is used by the embedded terminal. The optional `?contract=<name>` parameter starts the REPL on that contract (it must exist, otherwise the socket reports an error and closes).

## Related pages

- [Session commands](../commands/session.md)
- [Known Limitations](./limitations.md)
