use std::sync::Arc;

use rmcp::model::Tool;
use serde_json::{json, Map, Value};

pub fn build_tool_registry() -> Vec<Tool> {
    vec![
        tool(
            "ilold_project_overview",
            "List every project contract with its key (route name), qualified id, kind, source folder, function and state-variable counts, and inheritance. Dependency contracts (lib/) are loaded for resolution but not listed. Call this first to orient.",
            empty_schema(),
        ),
        tool(
            "ilold_project_map",
            "Full project topology: every contract with its functions and state variables, plus the resolved cross-contract call relationships.",
            empty_schema(),
        ),
        tool(
            "ilold_dependency_graph",
            "Contract dependency graph: inherits/calls/holds edges with topological reading-order layers. Shows which contracts are foundational and which depend on them.",
            empty_schema(),
        ),
        tool(
            "ilold_contract_dependencies",
            "Dependencies of one contract and its blast radius: the contracts it depends on, and the contracts that depend on it.",
            contract_schema(),
        ),
        tool(
            "ilold_contract_detail",
            "Full detail of one contract: functions (visibility, mutability, modifiers, params, path counts with paths_truncated, access level and guards), state variables, inheritance, and inherited members.",
            contract_schema(),
        ),
        tool(
            "ilold_entry_points",
            "Functions the contract runs, own and inherited (origin, is_inherited), with their access level (public, restricted, internal or special), guards read from code (kind CallerEquals/RoleCheck/TxOrigin for access, Pause/Reentrancy/Initializer for state; condition, via chain, span; heuristic when only a modifier name was available), state-write and external-call flags. Entries not internal are the externally reachable attack surface.",
            contract_schema(),
        ),
        tool(
            "ilold_search",
            "Searchable names in a contract: functions, state variables, events, and external-call targets.",
            contract_schema(),
        ),
        tool(
            "ilold_function_analysis",
            "Narrative of one function: access level and guards, paths (happy/revert; path_stats has the cutoffs and whether the enumeration was truncated; the first 100 paths are narrated and paths_omitted counts the rest), state reads and writes, external calls resolved to their real target, require checks, events, transitive effects, revert sources, and observations.",
            contract_function_schema(),
        ),
        tool(
            "ilold_trace",
            "Execution tree of a function with modifier bodies inlined and external calls resolved. depth limits internal-call inlining; reverts includes revert paths.",
            trace_schema(),
        ),
        tool(
            "ilold_callgraph",
            "Call graph of a contract as the most derived contract: own and inherited functions with super and virtual dispatch resolved. Edge kinds: Internal, InternalLibrary, LibraryDelegatecall, External, ExternalSelf (this.f), LowLevelCall, LowLevelDelegatecall, LowLevelStaticcall, Transfer, Send, Creation; node ids are Contract::name, or Contract::name(types) for overloads.",
            contract_schema(),
        ),
        tool(
            "ilold_cfg",
            "Control flow graph of one function: basic blocks and branch edges.",
            contract_function_schema(),
        ),
        tool(
            "ilold_function_paths",
            "Enumerated execution paths of one function, one page at a time: each with annotations (state writes, external calls, require checks) and terminal kind (Return/Revert/LoopCutoff/DepthCutoff). stats counts each terminal kind; truncated means a cutoff or the path cap (cap_reached, paths_dropped) left executions out. page.truncated means more paths follow: request the next offset.",
            paths_schema(),
        ),
        tool(
            "ilold_source",
            "Solidity source code of one function with its file path and line span.",
            contract_function_schema(),
        ),
        tool(
            "ilold_who_touches",
            "Functions that read and write a state variable, with their access level.",
            contract_variable_schema(),
        ),
        tool(
            "ilold_sequence_analysis",
            "Per-function behavior and the transition matrix of a contract: state shared between functions and conditions affected.",
            contract_schema(),
        ),
        tool(
            "ilold_sequences",
            "Transaction sequences of a contract up to a depth (public/external functions with a body; none for interfaces), one page at a time; page.total counts them all and page.truncated means more follow.",
            sequences_schema(),
        ),
        tool(
            "ilold_use",
            "Set the active contract for the session-scoped tools (session_call/state/back/clear, timeline, findings). Call before building a session.",
            contract_schema(),
        ),
        tool(
            "ilold_slice",
            "Backward and forward dataflow of a variable in a function of the active contract. A forward slice of a parameter is a taint analysis. Requires an active contract (ilold_use).",
            slice_schema(),
        ),
        tool(
            "ilold_session_call",
            "Add a function call to the active session sequence. Requires an active contract (ilold_use).",
            function_only_schema(),
        ),
        tool(
            "ilold_session_state",
            "Accumulated state changes across the active session. Requires an active contract (ilold_use).",
            empty_schema(),
        ),
        tool(
            "ilold_session_back",
            "Remove the last step from the active session. Requires an active contract (ilold_use).",
            empty_schema(),
        ),
        tool(
            "ilold_session_clear",
            "Reset the active session. Requires an active contract (ilold_use).",
            empty_schema(),
        ),
        tool(
            "ilold_timeline",
            "Mutations of a state variable across the steps of the active session. Build a session with session_call first.",
            variable_only_schema(),
        ),
        tool(
            "ilold_record_finding",
            "Record a security finding against the active session. Requires an active contract (ilold_use).",
            finding_schema(),
        ),
        tool(
            "ilold_note",
            "Attach a note to the current step of the active session. Requires an active contract (ilold_use).",
            note_schema(),
        ),
        tool(
            "ilold_set_status",
            "Set the review status of a function. Requires an active contract (ilold_use).",
            status_schema(),
        ),
        tool(
            "ilold_export",
            "Render the active session, findings and notes as a markdown report. Requires an active contract (ilold_use).",
            empty_schema(),
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Tool {
    Tool::new(name.to_string(), description.to_string(), Arc::new(to_object(schema)))
}

fn to_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn contract_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "contract": { "type": "string", "description": "Contract name, or its qualified id such as src/core/IToken.sol:IToken when several contracts share the name (an ambiguous name returns 409 with the candidate ids)" } },
        "required": ["contract"],
        "additionalProperties": false
    })
}

fn contract_function_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "contract": { "type": "string", "description": "Contract name, or its qualified id such as src/core/IToken.sol:IToken when several contracts share the name (an ambiguous name returns 409 with the candidate ids)" },
            "function": {
                "type": "string",
                "description": "Function name, or its signature such as setPoolPause(bool,uint40) when the name is overloaded (an ambiguous name returns 409 with the candidate signatures)"
            }
        },
        "required": ["contract", "function"],
        "additionalProperties": false
    })
}

fn contract_variable_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "contract": { "type": "string", "description": "Contract name, or its qualified id such as src/core/IToken.sol:IToken when several contracts share the name (an ambiguous name returns 409 with the candidate ids)" },
            "variable": { "type": "string", "description": "State variable name" }
        },
        "required": ["contract", "variable"],
        "additionalProperties": false
    })
}

fn trace_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "contract": { "type": "string", "description": "Contract name, or its qualified id such as src/core/IToken.sol:IToken when several contracts share the name (an ambiguous name returns 409 with the candidate ids)" },
            "function": { "type": "string", "description": "Function name" },
            "depth": { "type": "integer", "description": "Max internal-call inlining depth (default 2)" },
            "reverts": { "type": "boolean", "description": "Include revert paths (default false)" },
            "expand": { "type": "string", "description": "Comma-separated step ids to force-inline beyond depth, e.g. \"17,24\"" }
        },
        "required": ["contract", "function"],
        "additionalProperties": false
    })
}

fn paths_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "contract": { "type": "string", "description": "Contract name, or its qualified id such as src/core/IToken.sol:IToken when several contracts share the name (an ambiguous name returns 409 with the candidate ids)" },
            "function": { "type": "string", "description": "Function name, or its signature such as setPoolPause(bool,uint40) when the name is overloaded" },
            "offset": { "type": "integer", "description": "First path id of the page (default 0)" },
            "limit": { "type": "integer", "description": "Paths per page (default 100, at most 1000)" }
        },
        "required": ["contract", "function"],
        "additionalProperties": false
    })
}

fn sequences_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "contract": { "type": "string", "description": "Contract name, or its qualified id such as src/core/IToken.sol:IToken when several contracts share the name (an ambiguous name returns 409 with the candidate ids)" },
            "depth": { "type": "integer", "description": "Max sequence depth, 1 to 8 (default: the server's)" },
            "offset": { "type": "integer", "description": "First sequence of the page (default 0)" },
            "limit": { "type": "integer", "description": "Sequences per page (default 100, at most 1000; 0 returns only the functions)" }
        },
        "required": ["contract"],
        "additionalProperties": false
    })
}

fn function_only_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "function": { "type": "string", "description": "Function name" } },
        "required": ["function"],
        "additionalProperties": false
    })
}

fn variable_only_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "variable": { "type": "string", "description": "State variable name" } },
        "required": ["variable"],
        "additionalProperties": false
    })
}

fn slice_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "function": { "type": "string", "description": "Function name" },
            "variable": { "type": "string", "description": "Variable to slice" },
            "direction": { "type": "string", "enum": ["backward", "forward", "both"], "description": "Slice direction (default both)" }
        },
        "required": ["function", "variable"],
        "additionalProperties": false
    })
}

fn finding_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "severity": { "type": "string", "enum": ["Critical", "High", "Medium", "Low", "Informational"], "description": "Finding severity" },
            "title": { "type": "string", "description": "Short finding title" },
            "description": { "type": "string", "description": "Optional details" }
        },
        "required": ["severity", "title"],
        "additionalProperties": false
    })
}

fn note_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "text": { "type": "string", "description": "Note body" } },
        "required": ["text"],
        "additionalProperties": false
    })
}

fn status_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "function": { "type": "string", "description": "Function name" },
            "status": { "type": "string", "enum": ["Reviewed", "Suspicious", "Vulnerable", "Clean", "InProgress", "NotReviewed"], "description": "Review status" }
        },
        "required": ["function", "status"],
        "additionalProperties": false
    })
}
