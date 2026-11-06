# MCP Server

ilold ships a Model Context Protocol (MCP) server so an LLM agent can drive the analysis directly. The agent calls tools; ilold answers with the same resolved model the REPL and canvas use. Because the server analyzes the solc AST, the agent reasons over verified facts (cross-contract calls resolved to their real target, the dependency graph, access classification) instead of re-deriving them from raw source.

## How it connects

The MCP server is a thin stdio process that talks to a running `ilold serve` over its HTTP API. Two processes:

```
ilold serve <project> --port 8080          # the analysis server (and web canvas)
ilold evm-mcp --server-url http://127.0.0.1:8080   # the MCP server (stdio)
```

Registering it in an MCP client (Claude shown):

```
claude mcp add ilold-evm --transport stdio -- \
  ilold evm-mcp --server-url http://127.0.0.1:8080
```

Because the MCP drives the same server, the web canvas reflects what the agent does in real time: a function the agent inspects with `ilold_function_analysis` or `ilold_trace` is selected on the open page of that contract and marked in its session timeline, and session steps appear as the agent adds them. A human and an agent can audit the same protocol together.

## Tool conventions

- Tool names are prefixed `ilold_`.
- Analysis tools (orientation, structure, function, dataflow, sequence) do not change the analysis or any session's steps, and the same call on the same server returns the same result. Their side effects: `ilold_entry_points` and `ilold_who_touches` go through `POST /api/cmd`, so the contract they name becomes the server's current contract, and `ilold_function_analysis` and `ilold_trace` ask the server to highlight the function on the canvas (`POST /api/session/highlight` after the read succeeds).
- Session and findings tools mutate the active session. They operate on the contract set by `ilold_use`.
- Responses are shaped for context economy: overviews return names and counts and detail tools return one contract or one function. Nothing is summarized by a model: long lists are paged (`ilold_function_paths`, `ilold_sequences` take `offset` and `limit`), `ilold_function_analysis` narrates the first 100 paths and counts the rest in `paths_omitted`, and `ilold_trace` inlines calls down to `depth`. `ilold_project_map` and `ilold_callgraph` are returned whole and grow with the project.

## Tools

### Orientation (project level)

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_project_overview` | — | Contracts with kind, source folder, function and state-variable counts, inheritance. |
| `ilold_project_map` | — | Full topology: every contract, its functions and state variables, and the resolved cross-contract relationships, with dispatch candidates on external calls and the contract links (kind `ContractLink`). |
| `ilold_dependency_graph` | — | Contract dependency graph (`inherits`, `calls`, `holds`, `dispatches` with its contract link) with topological layers and cycles. |
| `ilold_contract_dependencies` | `contract` | Dependencies of one contract and its blast radius (what depends on it), contract links included. |

### Structure (contract level)

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_contract_detail` | `contract` | Functions (visibility, mutability, modifiers, params, path counts and `paths_truncated`), state variables, inheritance, inherited members. |
| `ilold_entry_points` | `contract` | Functions the contract runs, own and inherited (`origin`, `is_inherited`), with their access level (`Public`, `Restricted`, `Special`, or `Internal` for the ones not callable from outside) and guards read from code: `kind` (`CallerEquals`, `RoleCheck`, `TxOrigin`, or `AccessByName` when only a modifier name was available; `Pause`, `Reentrancy`, `Initializer` for state guards), `condition`, `via`, `span`, `heuristic`, `scope` (`Conditional` for a check on some paths only, which never restricts), `negated` (a deny-list, which never restricts), `requires_caller` (every way to pass needs a caller test that must hold), `compared_against`, `checked_by`, `role`, `role_implied_by`, `account`, `alternatives` and `conjuncts` (the parts of an `||` or `&&` access check, which carry these fields; the compound guard leaves them empty) and `ambiguous`. |
| `ilold_search` | `contract` | Searchable names in a contract: functions, state variables, events, external-call targets. |

### Function analysis

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_function_analysis` | `contract`, `function` | Narrative: paths (happy/revert, `path_stats` with cutoffs and truncation; the first 100 narrated, `paths_omitted` counts the rest), state reads and writes, resolved external calls, require checks, events, transitive effects, and observations. `accesses` lists the reads and writes of its own code with the mapping keys of each index level. |
| `ilold_function_paths` | `contract`, `function`, `offset`, `limit` (optional) | One page of the execution paths with per-path annotations and terminal kind, plus the path stats (cutoffs, `cap_reached`, `truncated`); `page.truncated` means more paths follow. |
| `ilold_trace` | `contract`, `function`, `depth` (optional), `reverts` (optional), `expand` (optional) | Execution tree with modifier bodies inlined and external calls resolved to their real target. |
| `ilold_callgraph` | `contract` | Function call edges of the contract as the most derived contract (own and inherited functions, `super` and virtual dispatch resolved), with call kinds (internal, library internal/delegatecall, external, `this`, low-level, transfer/send, creation) and call counts; external calls carry their dispatch candidates or unresolved reason and the address assignment sites. |
| `ilold_cfg` | `contract`, `function` | Control flow graph: basic blocks and branch edges. |
| `ilold_source` | `contract`, `function` | Source code of the function with its file path and line span. |

### Data flow

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_slice` | `function`, `variable`, `direction` (backward/forward/both) | Backward and forward dataflow of a variable in a function of the active contract. A forward slice of a parameter is a taint analysis. Requires `ilold_use`. |
| `ilold_who_touches` | `contract`, `variable` | Functions that read and write a state variable, with their access level. `accesses` gives each function's key sites (`msg_sender` keys a slot per caller, `state_var`, `literal` and `this` a shared one), or `keys_reason: "transitive"` when a callee makes the access. |
| `ilold_timeline` | `variable` | Mutations of a variable across the steps of the active session. Requires `ilold_use`. |

### Sequences (multi-function)

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_sequence_analysis` | `contract` | Per-function behavior and the transition matrix: state shared between functions and conditions affected. |
| `ilold_sequences` | `contract`, `depth`, `offset`, `limit` (optional) | Callable functions and one page of the transaction sequences up to a depth (none for interfaces); `limit` 0 returns only the functions. |

### Session and findings (stateful)

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_use` | `contract` | Set the active contract for session-scoped tools. |
| `ilold_session_call` | `function` | Add a function call to the session sequence; the canvas reflects the step. |
| `ilold_session_state` | — | Accumulated state changes across the session. |
| `ilold_session_back` | — | Remove the last step. |
| `ilold_session_clear` | — | Reset the session. |
| `ilold_record_finding` | `severity`, `title`, `description` (optional) | Record a finding against the current sequence. |
| `ilold_note` | `text` | Attach a note to the current step. |
| `ilold_set_status` | `function`, `status` | Set a function's review status. |
| `ilold_export` | — | Render the session, findings, and notes as a markdown report. |

## Recommended audit flow

1. `ilold_project_overview` and `ilold_dependency_graph` to orient: which contracts are foundational, which build on them.
2. `ilold_entry_points` per contract to find the externally callable surface and what is access-gated.
3. `ilold_function_analysis` and `ilold_trace` to study a function: its paths and its resolved external calls.
4. `ilold_use` to focus on a contract, then `ilold_slice` and `ilold_session_call` to trace the data flow of inputs and build attack sequences.
5. `ilold_contract_dependencies` to check the blast radius before judging a change.
6. `ilold_record_finding` and `ilold_export` to produce the deliverable.

## Notes

- The server must be running and reachable at `--server-url`.
- Most tools only read and take an explicit `contract`; a qualified id such as `src/core/IToken.sol:IToken` selects one of several contracts sharing a name. `ilold_slice` and `ilold_timeline` only read but operate on the active contract set with `ilold_use`; the session and findings tools also need it and mutate the session.
- Resolution has the same boundaries as the rest of the tool: calls through `address.call(...)` or assembly have no typed target and are not resolved. See [Known Limitations](./limitations.md).

## Related pages

- [Contract Commands](../commands/contract.md)
- [HTTP API](./api-endpoints.md)
- [Known Limitations](./limitations.md)
