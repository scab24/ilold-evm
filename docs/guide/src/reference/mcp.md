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

Because the MCP drives the same server, the web canvas reflects what the agent inspects in real time: the contract or function the agent is looking at is highlighted on the graph. A human and an agent can audit the same protocol together.

## Tool conventions

- Tool names are prefixed `ilold_`.
- Analysis tools (orientation, structure, function, dataflow, sequence) are read-only and deterministic; the same call always returns the same result.
- Session and findings tools mutate the active session. They operate on the contract set by `ilold_use`.
- Responses are shaped for context economy: overviews return names and counts, detail tools return one contract or one function, and large structures (paths, traces) are summarized rather than dumped.

## Tools

### Orientation (project level)

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_project_overview` | — | Contracts with kind, source folder, function and state-variable counts, inheritance. |
| `ilold_project_map` | — | Full topology: every contract, its functions and state variables, and the resolved cross-contract relationships. |
| `ilold_dependency_graph` | — | Contract dependency graph (`inherits`, `calls`, `holds`) with topological layers and cycles. |
| `ilold_contract_dependencies` | `contract` | Dependencies of one contract and its blast radius (what depends on it). |

### Structure (contract level)

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_contract_detail` | `contract` | Functions (visibility, mutability, modifiers, params, path stats), state variables, inheritance, inherited members. |
| `ilold_entry_points` | `contract` | Externally callable functions with their access level (public, or restricted to a role). |
| `ilold_search` | `contract` | Searchable names in a contract: functions, state variables, events, external-call targets. |

### Function analysis

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_function_analysis` | `contract`, `function` | Narrative: paths (happy/revert), state reads and writes, resolved external calls, require checks, events, transitive effects, and observations. |
| `ilold_function_paths` | `contract`, `function` | Enumerated execution paths with per-path annotations and terminal kind (return/revert). |
| `ilold_trace` | `contract`, `function`, `depth` (optional), `reverts` (optional), `expand` (optional) | Execution tree with modifier bodies inlined and external calls resolved to their real target. |
| `ilold_callgraph` | `contract` | Function call edges within and out of the contract (internal, external, inherited) with call counts. |
| `ilold_cfg` | `contract`, `function` | Control flow graph: basic blocks and branch edges. |
| `ilold_source` | `contract`, `function` | Source code of the function with its file path and line span. |

### Data flow

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_slice` | `function`, `variable`, `direction` (backward/forward/both) | Backward and forward dataflow of a variable in a function of the active contract. A forward slice of a parameter is a taint analysis. Requires `ilold_use`. |
| `ilold_who_touches` | `contract`, `variable` | Functions that read and write a state variable, with their access level. |
| `ilold_timeline` | `variable` | Mutations of a variable across the steps of the active session. Requires `ilold_use`. |

### Sequences (multi-function)

| Tool | Parameters | Returns |
| --- | --- | --- |
| `ilold_sequence_analysis` | `contract` | Per-function behavior and the transition matrix: state shared between functions and conditions affected. |
| `ilold_sequences` | `contract`, `depth` (optional) | Transaction sequence tree up to a depth. |

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
- Most tools are read-only and take an explicit `contract`. `ilold_slice` and `ilold_timeline` only read but operate on the active contract set with `ilold_use`; the session and findings tools also need it and mutate the session.
- Resolution has the same boundaries as the rest of the tool: calls through `address.call(...)` or assembly have no typed target and are not resolved. See [Known Limitations](./limitations.md).

## Related pages

- [Contract Commands](../commands/contract.md)
- [HTTP API](./api-endpoints.md)
- [Known Limitations](./limitations.md)
