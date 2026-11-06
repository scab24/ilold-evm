# HTTP API Reference

ilold exposes an HTTP API on the server port: `ilold serve --port` (default 8080), or a free port `ilold explore` picks and prints. Both listen on 127.0.0.1 unless `--host <addr>` says otherwise; `--host 0.0.0.0` exposes the API, including `/ws/pty`, to the network. Endpoints return JSON except `/ws` and `/ws/pty`. The API is split into the command bus, session queries, contract queries, annotations and WebSockets.

## Security

Every request, WebSockets included, must pass these checks or it gets `403 Forbidden`:

- **Host** (against DNS rebinding): the `Host` header must carry the server port and name `localhost`, a loopback address (`127.0.0.1`, `[::1]`) or the `--host` address. With `--host 0.0.0.0` or `--host ::` it may also name the interface address the connection arrived on, for example `http://192.168.1.5:8080` from another machine. Host names other than `localhost` are refused, so reach a remote server by IP address.
- **Origin** (against cross-site requests): a request without an `Origin` header (the REPL, the MCP server, curl) passes; a browser request passes only when its `Origin` is the server itself (scheme `http` or `https` and the same host and port as `Host`). Other pages cannot open `/ws/pty`, post to `/api/cmd` or change annotations.
- **Sec-Fetch-Site** (against cross-site reads such as `<img>` or `<script>`, which carry no `Origin`): `cross-site` and `same-site` are refused; `same-origin`, `none` (a typed URL or bookmark) and a missing header (non-browser clients) pass.

`GET` routes only read: they never change sessions, notify other clients or start processes. The one exception is the `/ws/pty` WebSocket upgrade, which must be a `GET` and starts the embedded terminal's REPL only after the checks above.

The API sends no CORS headers; the web UI is served from the same origin. In frontend development, the Vite dev server proxies `/api` and `/ws` to port 8080 with the backend's own `Host`; start the backend with `ILOLD_DEV_ORIGIN` naming the dev page so its `Origin` is accepted:

```
ILOLD_DEV_ORIGIN=http://localhost:5173 ilold serve <path> --port 8080
cd crates/ilold-web/frontend && npm run dev
```

## Command bus

### POST /api/cmd

Execute a session command. This is the single entry point for all state-mutating session operations.

Each contract owns an independent session store (scenarios, forks and steps). A command only touches the store of the contract it names, and that contract becomes the server's **current contract**. Naming another contract never discards the previous contract's session. At startup the current contract is the project default: the most derived deployable contract (abstract contracts, libraries and interfaces only as fallbacks), ties broken by name. Tests, scripts, mocks and harnesses (contracts under the configured test or script directories, under a `mock(s)`, `test(s)`, `harness(es)` or `script(s)` directory, or named `Mock*`, `Test*` or `Harness*`) are chosen only when nothing else is. `explore` uses the same rule.

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

- `StepAdded`: `step_index`, `function`, `access`, `state_changed` (list of variable names, including writes made through calls such as `_reserves[].liquidityIndex`)
- `StateView`: `summary` (list of variable summaries with writers per step)
- `FunctionList`: `functions` (name, access level, writes_state, has_external_calls)
- `VariableInfo`: `variable`, `writers` (name + access), `readers` (name + access)
- `Error`: `message`

## Session query endpoints

These endpoints read the active scenario of one contract's session. They never change which contract is current.

Every endpoint in this section, plus `GET /api/scenarios` and `GET /api/scenarios/all`, accepts an optional `?contract=<name>` query parameter:

- With `contract`, the endpoint reads that contract's session. An unknown contract returns 404 and a bare name shared by several contracts 409, as on the contract routes. A contract with no commands yet reads as an empty `main` scenario.
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

**Response:** `FunctionNarrative` with paths, state writes, external calls, and conditions; paths are capped as in `GET /api/session/function/{contract}/{func}`.

### GET /api/session/step/{index}/trace

Returns the persisted FlowTree of a session step. This is the tree captured when `Call` was executed, not a recomputation.

**Path params:** `index` -- zero-based step index.

**Response:** `FlowTree` with step nodes, each containing operation type, target, conditions, and child steps. Returns 404 if the step has no persisted tree (sessions saved without one).

### GET /api/session/timeline/{variable}

Returns a chronological timeline of every write to `variable` across all session steps. Matches by base name (e.g., `balances` matches `balances[msg.sender]`).

**Path params:** `variable` -- state variable name.

**Response:** `VariableTimeline` with `state_entries` and `local_entries`. Each entry contains: `session_step_index`, `function`, `target`, `operator`, `value_expr`, `reached_when` (path conditions), `via` (modifier name if applicable), `scope`.

### GET /api/session/slice/{function}/{variable}

Returns a dataflow slice for `variable` inside `function` as the contract runs it: an own or inherited function, with modifiers resolved as the contract sees them.

**Path params:** `function` -- function name or signature, `variable` -- variable name.

**Query params:**

| Param | Values | Default | Description |
|---|---|---|---|
| `direction` | `backward`, `forward`, `both`, `b`, `f`, `all` | `both` | Slice direction |
| `contract` | contract name | current contract | Contract whose function is sliced |

**Response:** `SliceResult` with `backward` and `forward` arrays. Each entry contains: `path`, `span` (source location), `text` (rendered statement), `origin` (`FunctionBody` or `Modifier(name)`).

### GET /api/session/trace/{contract}/{func}

Returns a FlowTree for the given function, computed on demand from the current analysis data.

**Path params:** `contract` -- contract name (404 unknown, 409 ambiguous), `func` -- function name.

**Query params:**

| Param | Type | Default | Description |
|---|---|---|---|
| `depth` | integer | 2 | Maximum inline depth for internal calls |
| `reverts` | boolean | false | Include revert paths in the tree |
| `expand` | string | empty | Comma-separated step IDs to force-inline beyond max depth (e.g., `17,24`) |

**Response:** `FlowTree` with nested step nodes representing the execution flow, and `path_stats` (see [Path stats](#path-stats)) for the traced function.

### GET /api/session/function/{contract}/{func}

Returns a function narrative without requiring a session step. Useful for inspecting functions that have not been called in the session.

**Path params:** `contract` -- contract name (404 unknown, 409 ambiguous), `func` -- function name.

**Response:** `FunctionNarrative` with paths, state effects, and behavioral summary. `path_stats` holds the [path stats](#path-stats); the response narrates at most the first 100 paths step by step and `paths_omitted` counts the rest (every path is in `GET /api/contract/{name}/{func}/paths`).

### POST /api/session/highlight

Selects a function on every canvas showing the contract, through a `session_highlight` [WebSocket](#get-ws) message. The REPL's `info`, `trace` and `step` and the MCP's `ilold_function_analysis` and `ilold_trace` send it after a successful read; the reads themselves never affect other clients.

**Request body:** `{"contract": "Vault", "function": "deposit"}` or `{"contract": "Vault", "step": 0}`. `contract` is optional (the current contract) and goes in the body, not the query; give exactly one of `function` (name or signature, inherited functions included) and `step` (an index into the contract's active scenario).

**Response:** `{"contract": ..., "function": ...}` with the contract key and the function's key (its name, or its signature when the name is overloaded). 400 without exactly one target, 404 for an unknown function or step.

## Contract query endpoints

These endpoints read from the static analysis data and do not require an active session.

### Contract identity and dependencies

Every contract has a stable `qualified_id`, `<source path relative to the project root>:<Name>` (for example `src/contracts/interfaces/IAToken.sol:IAToken`), and a `name` used as `{name}` in the routes below: the bare Solidity name when no other project contract shares it, otherwise the qualified id. `display_name` is the Solidity name as declared. `{name}` accepts the key, the qualified id, or a bare name that selects exactly one contract; a bare name shared by several contracts returns `409 Conflict` with `{"error": ..., "candidates": ["src/a/T.sol:T", "src/b/T.sol:T"]}`. Encode the `/` of a qualified id as `%2F` in a URL.

Contracts from libraries and vendored code (`lib/`, `src/**/dependencies/`) that the project sources import are part of the model so that inheritance, modifiers, `super` and calls resolve, and are flagged `is_dependency: true`. Listings show project contracts only; `?include_dependencies=true` adds the dependencies on `/api/project`, `/api/project/map` and `/api/project/depgraph`. Dependency contracts get CFGs and path trees only when a project contract inherits them; they have no call graph or sequences of their own.

### GET /api/project

Returns a project summary: file count, the listed contracts (`name`, `display_name`, `qualified_id`, `is_dependency`, kind, function count, state variable count, `inherits` with the base names as written and `inherit_keys` with the same bases as `{name}` route segments, omitting bases outside the model), the number of `dependency_contracts` in the model, the number of `hidden_contracts` default listings omit (dependencies and free-function containers), `warnings`, and the `default_contract` explore and the server start on. A warning is reported when test or script files fail to compile: the model is then built from the sources alone and the warning names the failing files. Errors in the sources still fail the analysis.

### GET /api/project/map

Returns the full project map with the listed contracts (with `inherits` and `inherit_keys`), their functions (with path counts, `paths_truncated` and external call flags), state variables, and cross-contract relationships extracted from call graphs. A call relationship carries `call_count` and, for an external call to a contract, `dispatch_candidates` (or `unresolved_reason`), as in the [graph export](graph-export.md#dispatch-candidates). Each contract link adds a relationship of kind `ContractLink` with `from_function` and `to_function` `null`, `basis`, `tag`, `via_state_vars` (names) and `call_count` (the calls it aggregates).

### GET /api/project/depgraph

Returns the contract dependency graph in Cytoscape-compatible JSON. Nodes carry the contract `name` as id, `qualified_id`, `is_dependency`, kind, source folder, and topological layer. Edges carry the relationship kinds (`inherits`, `calls`, `holds`, `dispatches`) and call count, and `link`: the [contract link](graph-export.md#contract-links) when the target may answer calls the source makes through an interface (`basis`, `tag`, `via_state_vars`, and the `calls` with their function and span), else `null`; inheritance edges follow the declaration solc resolved, so an aliased or same-named base reaches the right contract.

### GET /api/contract/{name}

Returns contract detail: name, `display_name`, `qualified_id`, `is_dependency`, kind, direct bases (`inherits` as written, `inherit_keys` as route segments, so an aliased or same-named base links to the right contract; bases outside the model are omitted), functions (with `path_count`, `happy_paths`, `revert_paths` and `paths_truncated`), state variables, and inherited functions and state variables. Inherited functions are the public, external and internal functions of the linearization that the contract does not override; private functions and base constructors are not inherited, and an interface function implemented by a public state variable getter is not listed. Inherited state variables are in storage layout order, most base contract first (slot numbers are not computed). `outgoing_links` and `incoming_links` list the contract links from and to the contract. Each function carries its `signature` and the `key` to use as `{func}` in the routes below: the bare name when it is unique on this contract, otherwise the signature.

Signatures are `name(type,...)`:

- External and public functions use the ABI form, without data locations: `sums(uint256[])`.
- Internal and private functions keep each parameter's location, because overloads may differ only by it: `_sum(uint256[] calldata)`, `_sum(uint256[] storage)`.
- `struct`, `enum` and `contract` prefixes are dropped (`DataTypes.ReserveData`, `IPool`). Function-type parameters are written `function(uint256) external returns (bool)`.
- The receive and fallback functions have the bare signatures `receive` and `fallback`. A regular function with one of those names (solc warns about it) is always keyed by its signature, e.g. `fallback()`.

A bare-name key is not stable: adding an overload later turns an existing key such as `put` into `put(uint256)`. Clients that store function references should prefer the signature.

### GET /api/contract/{name}/callgraph

Returns the call graph for a contract in Cytoscape-compatible JSON format, built as the most derived contract: its own and inherited functions, with `super` following the C3 linearization and calls to virtual functions dispatched to the visible override. Node ids are `Contract::name`, or `Contract::name(types)` for overloaded functions; `is_external` marks nodes outside the contract's hierarchy. Nodes of functions the contract runs carry the `key` to use as `{func}` in the function routes. Edges carry the call `kind`, a `kind_label`, the call count and, for dispatched calls, the `static_target` named in the source. `External` and `ExternalSelf` edges also carry `dispatch_candidates` (or `unresolved_reason`), which depend on the target only, and `assigned_at`: the assignment sites of the state variables holding the address, over every call site the edge counts. Kinds:

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

`{func}` in `/api/contract/{name}/{func}/cfg`, `/paths` and `/source` is a bare function name when it is unambiguous on the contract (own and inherited functions), or a full signature such as `setPoolPause(bool,uint40)`. Whitespace in a signature is ignored, so `_sum(uint256[]storage)` matches `_sum(uint256[] storage)`. A bare name shared by several overloads returns `409 Conflict` with `{"error": ..., "candidates": ["setPoolPause(bool,uint40)", "setPoolPause(bool)"]}`. All three routes select the same overload for the same `{func}`.

### GET /api/contract/{name}/{func}/cfg

Returns the control flow graph for a function in Cytoscape-compatible JSON format. Nodes represent basic blocks (entry, normal, return, revert, assembly, loop) with their `statements` and, when known, the `span` of source lines they run (`file_index`, `start_line`, `start_col`, `end_line`, `end_col`) and the `file` path it points into: the function's own lines, or the modifier's for a block of inlined modifier code; a revert or return block cites the statement that ends there. Edges represent control flow transitions.

### GET /api/contract/{name}/{func}/source

Returns the function's source text with its `file_path` and `span`. For an inherited function this is the declaring contract's code.

### Pages

Long lists are served one page at a time. `?offset=` (default 0) selects the first item and `?limit=` the page size (default 100, at most 1000; a larger limit is clamped). The response carries `page`: `{"offset", "limit", "total", "truncated"}`, where `limit` is the size applied, `total` counts every item and `truncated` is true when items follow the page.

### Path stats

Path enumeration cuts a path off when it reaches a loop condition a fourth time (`LoopCutoff`) or more than 32 blocks after the entry (`DepthCutoff`), and stops after 10,000 paths. Every path ends in one category, so `happy_paths` (return) + `revert_paths` + `loop_cutoff` + `depth_cutoff` = `total_paths`. `cap_reached` means the 10,000-path cap stopped the walk with `paths_dropped` unexplored branches left, each holding at least one more path. `truncated` is true when any path was cut off or dropped: the paths then do not cover every execution. `max_depth_reached` is the longest path seen.

### GET /api/contract/{name}/{func}/paths

Returns one [page](#pages) of the function's paths, by id, with the [path stats](#path-stats) of the whole tree and a `truncation_note` naming the causes when the paths are truncated (otherwise `null`). Each path has its nodes, terminal kind (`Return`, `Revert`, `LoopCutoff`, `DepthCutoff`) and annotations (state writes, external calls, events); a cut-off path stops before running its last node. `internal_calls` lists JUMPs (`f`, `super.f`, `Base.f`, `Library.f` for internal library functions), and `internal_call_targets` repeats them in the same order with the `declaration` solc resolved (`[contract, key]`). `external_calls` lists everything that leaves the frame, each with its call `kind`.

### GET /api/contract/{name}/sequences

Returns the functions a transaction sequence can call (public and external functions with a body; none for an interface) and one [page](#pages) of the sequences of 1 to `?depth=` calls (1 to 8, default the server's `--max-seq-depth`), shortest first and each depth in lexicographic order. There are `n + n^2 + ... + n^depth` of them for `n` functions; they are generated per request. `?limit=0` returns only the functions. A depth outside 1 to 8 is `400 Bad Request`.

### GET /api/contract/{name}/analysis

Returns the sequence analysis for a contract: per-function behavior summaries (state writes, state reads, external calls, conditions) and inter-function transition information. It is computed with the project analysis; a dependency contract, which has none of its own, gets one from its path trees on request. An unknown contract is 404, an ambiguous name 409.

### GET /api/contract/{name}/suggestions

Returns search suggestions for the contract: function names (own and inherited), state variable names, event names, external call targets (sorted), and predefined categories (revert, return, assembly).

## Annotations

Free-form notes on the canvas, held in memory for the server's lifetime (not part of saved sessions). An annotation has `id` (required, may be empty), `contract`, `text`, `severity` (`info`, `low`, `medium`, `high`, `critical`) and `status` (`open`, `reviewed`, `finding`), all required, and optional `function`, `path_id` and `block_id`.

| Route | Description |
|---|---|
| `GET /api/annotations` | Every annotation |
| `POST /api/annotations` | Create one; an empty `id` is assigned (`ann-N`). Returns `201 Created` with the annotation |
| `PUT /api/annotations/{id}` | Takes a full annotation body and applies its `text`, `severity` and `status`; 404 for an unknown id |
| `DELETE /api/annotations/{id}` | `204 No Content`, or 404 for an unknown id |

## WebSocket

### GET /ws

Upgrades to a WebSocket connection. The server pushes JSON messages when session state changes. Every session message carries the `contract` whose session changed, so clients ignore messages for other contracts:

- `session_add_node`: a step was added (scenario, function, access level, step index)
- `session_remove_node`: the last step was removed
- `session_clear`: all steps were cleared
- `session_highlight`: a client asked to show `function` (scenario, and the function's key: its name, or its signature when the name is overloaded) through [`POST /api/session/highlight`](#post-apisessionhighlight); canvases showing the contract select it
- `scenario_created`, `scenario_switched`, `scenario_deleted`, `scenario_forked`, `scenario_store_reloaded`: scenario lifecycle events
- `search_result`, then `search_complete` with `total` and `truncated`: the answer to a `{"type": "search", "query": ..., "contract": ..., "function": ...}` message (`contract` and `function` optional) streams the first 100 matching paths, `total` counts every match. Each result names the `function` as its declaring contract keys it and the `key` to use as `{func}` for the result's `contract`. With `contract`, the search covers the functions it runs, inherited ones included, and reports them under that contract; an unknown or ambiguous `contract` gets an `error` message instead
- `session_resync` (no `contract`): the client fell behind and messages were dropped; refetch the session with `GET /api/scenarios/all?contract=<name>`

### GET /ws/pty

Upgrades to a WebSocket bridged to an `ilold explore --attach` REPL, which is used by the embedded terminal. A request from another origin is refused before any REPL starts (see [Security](#security)). The optional `?contract=<name>` parameter starts the REPL on that contract (it must exist, otherwise the socket reports an error and closes).

## Related pages

- [Session commands](../commands/session.md)
- [Known Limitations](./limitations.md)
