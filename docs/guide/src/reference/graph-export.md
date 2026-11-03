# Graph Export

`ilold graph --json <out> [path]` writes the facts ilold derives from a Foundry project as JSON Lines: one JSON object per line. It runs offline and reuses the project and analysis caches of the other commands.

```
ilold graph --json map.jsonl .
```

`path` defaults to `.` and may be any directory or file inside the project; the Foundry root is the nearest directory with a `foundry.toml`.

## Exit codes

| Code | Meaning | Output file |
| --- | --- | --- |
| `0` | Complete. | Written. |
| `1` | Tool error: not a Foundry project, unreadable `foundry.toml`, unwritable output. | Not written. |
| `2` | Invalid command-line usage. | Not written. |
| `3` | Complete with warnings: files outside the sources (tests, scripts) failed to compile. They are listed as `failed_compile`; everything else is exported. | Written. |
| `4` | No facts: a compile error inside the sources. | Only the envelope, with the file inventory. |

`analyze` exits `1` on any compile failure. `graph` exits `4` for a compile error inside the sources, so a consumer can tell "nothing to map" from "the tool failed".

The output is written to a temporary file next to `<out>` and renamed into place once complete, so a crash never leaves a partial file.

## Determinism

The same commit, the same `foundry.toml` profile and the same ilold version produce byte-identical output, whether the caches are cold or warm.

- No timestamps. Every path is relative to the git root (`git rev-parse --show-toplevel` run from the Foundry root); outside a git repository, to the Foundry root.
- No absolute paths, so a copy of the project in another directory exports the same bytes. Absolute paths inside compiler messages (`files[].error`, `warnings[].message`) are rewritten: under the root, symlinks such as `/tmp` → `/private/tmp` resolved, they become root-relative (`.` for the root itself); anywhere else they become `<outside-root>/<file name>`.
- Every line is compact JSON with its keys sorted.
- Line 1 is the envelope. Nodes follow, then edges; each part is sorted by `id`. Ids are unique, so the order needs no tie-break.

## Envelope

| Field | Meaning |
| --- | --- |
| `kind` | `"envelope"`. |
| `schema_version` | `"1.0.0"`. |
| `tool` | `{name, version, git_commit, dirty}`. `git_commit` is the commit ilold was built from and `dirty` whether its sources (`crates/`, `Cargo.toml`, `Cargo.lock`, untracked files included) differed from it; both are `null` for a build outside a git checkout. |
| `target` | `{git_commit, dirty, sources_sha256}`. `git_commit` and `dirty` are `null` outside a git repository; `git_commit` is also `null` before the first commit. `dirty` covers the Foundry root, untracked files included. `sources_sha256` hashes every analysed source in path order: `path`, a NUL byte, the SHA-256 of its content, a newline. |
| `project_root` | The Foundry root relative to the git root; `""` when they are the same directory. |
| `solc_versions` | Every solc version that compiled an analysed source. Empty when the sources failed to compile. |
| `profile` | The Foundry profile (`FOUNDRY_PROFILE`, else `default`). |
| `remappings_sha256` | SHA-256 of the remappings solc received, one `context:name=path` per line, paths relative to the Foundry root. |
| `status` | `complete`, `complete_with_warnings` or `no_facts` (exit codes 0, 3 and 4). |
| `files` | The file inventory, below. |
| `warnings` | `{kind: "partial_compile", message}` when files outside the sources failed; `{kind: "compile_error", message}` for a compiler error without a file; then one `{kind: "empty_dependency", message: "<dir> is empty (git submodule not initialised?)"}` per directory, sorted by path. `<dir>` is the first empty directory on the way to a remapping target, or a submodule declared in the git root's `.gitmodules` that is empty or missing. A missing remapping target that no submodule declares gives no warning. These warnings do not change `status`. |
| `truncation` | Where the analysis stopped early, below. |
| `counts` | Number of records per `kind`. |

### File inventory

Every `.sol` file under the Foundry root (build, cache, `node_modules` and VCS directories skipped) and every source the model loaded from elsewhere, sorted by path. A file that fails or is left out never looks like a file without facts.

| Field | Meaning |
| --- | --- |
| `path` | Relative to the git root. |
| `status` | `analysed` (project source), `dependency` (library or vendored code the sources import), `test_or_script` (under the test or script directory and imported by no source), `excluded` (anything else), `failed_compile`. |
| `reason` | Why the file has no facts: `no project source imports it`, or `a compile error inside the sources left no facts` (every file but the failing ones when the status is `no_facts`). |
| `error` | For `failed_compile`: the compiler errors, `<type> <code>: <message>`, joined by `; `. |
| `line` | For `failed_compile`: the line of the first error. |

### Truncation

Counts of where the analysis stopped, and the limits it used. Exported facts never depend on the path limits: effects come from every reachable block of a function, not from enumerated paths.

| Field | Meaning |
| --- | --- |
| `effects_incomplete`, `reads_incomplete`, `reverts_incomplete`, `guards_incomplete` | Function and context pairs with the flag set (see `analysis` on function nodes). |
| `paths_truncated`, `depth_cutoffs`, `loop_cutoffs`, `paths_dropped` | Over the exported functions' path trees. |
| `limits` | `max_depth`, `max_loop_unroll`, `max_paths` (path enumeration), `guard_depth` (helper calls the guard walk follows), `effect_rounds` (rounds before a recursive cycle's effects are cut). |

## Ids

| Declaration | Id |
| --- | --- |
| Contract, interface, library | `evm:<path>:<Contract>` |
| Function | `evm:<path>:<Contract>.<signature>` |
| Free function | `evm:<path>:<signature>` |
| State variable, modifier | `evm:<path>:<Contract>.<name>` |
| Event, error | `evm:<path>:<Contract>.<name>`, or `.<name>(<types>)` when the contract declares the name more than once |
| File-level event, error | `evm:<path>:<name>`, with `(<types>)` when the file declares the name more than once |

`<path>` is the declaring file relative to the git root. A function signature is its name and parameter types: `deposit(uint256)`. Contract types appear by name (`setFeed(IFeed)`). Internal and private functions keep data locations, because overloads may differ only by them: `_sum(uint256[] storage)` and `_sum(uint256[] memory)`. `receive()`, `fallback()` and `constructor(...)` follow the same form. The `selector` field carries the ABI selector where one exists.

Edge ids are `<kind>:<from>#<hash>.<n>`. The hash (16 hex digits) covers the record without `span`, `tag`, `rule` and `id`, so it never depends on positions: moving unrelated code keeps every id. `<n>` numbers, from `0`, the edges of `<from>` that share the hash, ordered by span (file, then line and column); it is always present, so a first occurrence keeps `.0` when another one is added later. Equal facts, position included, collapse into one line.

## Common fields

Every node and edge has:

| Field | Meaning |
| --- | --- |
| `id` | As above. |
| `kind` | The record kind. |
| `span` | `{file, start_line, start_col, end_line, end_col}`, 1-based, `file` relative to the git root. |
| `tag` | How the fact was established, below. |
| `rule` | The ilold pass that produced it, below. |

### Tags

| Tag | Meaning |
| --- | --- |
| `EXTRACTED` | Read from the syntax: declarations, reverts, code facts, guards found in code on every path, storage-parameter accesses. |
| `TYPE_RESOLVED` | Resolved through solc declaration ids and types: call targets, emitted events, state variables of reads and writes, linearization. |
| `INFERRED` | A heuristic: guards from a modifier name, guards on some paths only (`scope: conditional`), events matched by name where solc gave no id, calls resolved by name, linearization without solc data. |
| `AMBIGUOUS` | Unresolved: the edge keeps its text and `to`, `event` or `state_var` is `null`. A modifier without code is `AMBIGUOUS` too. |

### Rules

| Rule | Pass |
| --- | --- |
| `parse` | The solc AST model: every node. |
| `linearization` | C3 linearization and name lookup over it: `inherits`, `exposes`, `uses_modifier`. |
| `cfg` | The function's control-flow graph, modifiers inlined: direct `calls`, `reads`, `writes`, `emits`, `reverts`, `opaque_assembly`. |
| `effects` | Effects of the functions it calls, which run on its storage: facts with a non-empty `via`. |
| `guards` | The guard walk over the function, its modifiers and helpers. |
| `code_facts` | Builtins and unchecked blocks, modifiers included. |

## Contexts

One node per declaration. What a function does depends on the contract that runs it (virtual dispatch, modifier overrides, the storage it touches), so every fact about a function carries `in_context`: the id of that contract.

- Every function a deployable project contract (a `contract`, not abstract) can run is exported in that contract's context, inherited and dependency functions included.
- A function no deployable contract runs (abstract and library code, private helpers of a base, dependency code project code calls) is exported in its own contract.
- Free functions have `in_context: null`.

### Dependencies

Every dependency contract gets a `contract` node (`is_dependency: true`). Its members get nodes only when project code can reach them: all members of a dependency a project contract inherits, otherwise only the members an edge refers to. Dependency functions the project calls are analysed like project code. `reachable_from_project` is `true` for project contracts and for dependencies with exported members.

## Nodes

| Kind | Fields |
| --- | --- |
| `contract` | `name`, `contract_kind` (`contract`, `abstract`, `interface`, `library`), `bases` (linearized, most derived first, itself excluded), `is_dependency`, `deployable`, `reachable_from_project`. |
| `function` | `name`, `contract` (`null` for free functions), `function_kind` (`function`, `constructor`, `fallback`, `receive`), `visibility`, `mutability` (`pure`, `view`, `payable`, `non_payable`), `selector`, `signature`, `modifiers` (names, in order), `virtual`, `override`, `overrides` (the declaration it overrides, next in the linearization), `is_entry_point` (public or external, not a constructor), `has_body`, `paths` (path-tree statistics, `null` if not analysed), `analysis` (per context: `{in_context, effects_incomplete, reads_incomplete, reverts_incomplete, guards_incomplete}`). |
| `state_var` | `name`, `contract`, `type`, `visibility`, `constant`, `immutable`, `order` (declaration order in its contract), `selector` (when it implements an interface function). |
| `event`, `error` | `name`, `contract` (`null` at file level), `params` (types). |
| `modifier` | `name`, `contract`, `params` (types), `has_body`. |

`effects_incomplete`: assembly that may change state runs, or a recursive cycle was cut. `reads_incomplete`: assembly in a view function runs, or effects are incomplete. `reverts_incomplete`: assembly runs, and Yul can revert. `guards_incomplete`: a helper chain deeper than `guard_depth` was cut.

## Edges

Fact edges (everything but `inherits` and `exposes`) carry `function` (or `from` for `calls`) and `in_context`. `via` lists the ids from the function to the code with the fact: a modifier, then the functions called. Every fact in modifier code, reads and reverts included, names the modifier. An element that is not an `evm:` id is the analysis label it could not resolve.

| Kind | Fields |
| --- | --- |
| `inherits` | `child`, `base`, `order` (1-based position in the child's linearization), `direct` (declared in `is`). |
| `exposes` | `contract`, `function` (a function, or a state variable for a getter), `via_base` (the declaring base when inherited, else `null`), `selector`, `getter`. One per externally callable function of each deployable project contract. |
| `calls` | `from`, `to` (`dispatch_target`, else `static_target`, else `null`), `target_text`, `call_kind`, `static_target` (the declaration the code names), `dispatch_target` (what runs in this context: the virtual override, the `super` target), `options` (`{value, gas, salt}` as written), `target_source`, `result_used` (low-level calls and `send` only), `via`. |
| `reads`, `writes` | `state_var`, `path` (`balances[]`, `s.x`), `param` (the storage parameter it goes through, then `state_var` is `null`), `via`. |
| `emits` | `event` (the declaration solc resolved; without an id, the one event with that name, else `null`), `event_text`, `via`. An emit reached through a call yields one edge per event declaration the callee emits under that name. |
| `reverts` | `reason` (`require(cond)`, `revert Err(args)`, ...), `via`. |
| `guards` | `guard_kind` (`caller_equals`, `role_check`, `tx_origin`, `access_by_name`, `pause`, `reentrancy`, `initializer`), `condition_text`, `scope` (`unconditional`, `conditional`), `restricts`, `negated`, `compared_against` (caller and `tx.origin` comparisons only), `checked_by` (the function or mapping a role check consults), `role` (the role argument), `role_implied_by` (the predicate that names the role when no argument does, as `aclManager.isPoolAdmin` in `aclManager.isPoolAdmin(msg.sender)`), `account` (whose access is checked: `msg_sender`, `tx_origin`), `alternatives`, `conjuncts`, `heuristic`, `via`. See [Guard conditions](#guard-conditions). |
| `uses_modifier` | `modifier` (its declaration in this context, `null` if none), `name`, `order`. |
| `builtin_use` | `name`: `ecrecover`, `selfdestruct`, `tx.origin`, `block.timestamp`, `block.number`, `blockhash`, `code.length`, `gasleft`, `msg.value`, `balance`, `block.chainid`, `abi.encodePacked`. |
| `unchecked_block` | The span of the block. |
| `opaque_assembly` | The span of the assembly block, `via`. See `effects_incomplete`. |

`call_kind` is `internal`, `internal_library`, `library_delegatecall`, `external`, `external_self`, `low_level_call`, `low_level_delegatecall`, `low_level_staticcall`, `transfer`, `send` or `creation`. Builtins are never calls. A call whose target is unknown, a low-level call to an address included, has `to: null` and tag `AMBIGUOUS`.

`target_source` (the receiver of a call to an address), `compared_against` (what a guard compares the caller with), `role` and `account` are one of:

| `kind` | Fields |
| --- | --- |
| `state_var`, `immutable`, `constant` | `name`, `id` |
| `param` | `index`, `name` |
| `literal` | `text` |
| `msg_sender`, `tx_origin` | — |
| `expression` | `text` |

So `require(msg.sender == owner)` compares against `{kind: "state_var", name: "owner", id: ...}` and `require(msg.sender == to)` against `{kind: "param", index: 0, name: "to"}`.

### Guard conditions

An access guard's condition is split into terms. A term is `{guard_kind, condition_text, negated, compared_against, checked_by, role, role_implied_by, account, alternatives, conjuncts}`, `condition_text` being its part of the condition:

- `alternatives` lists every disjunct of an `||`, `conjuncts` every conjunct of an `&&`, in source order; a chain of the same operator is one list. Each part is itself a term, so a mix of `&&` and `||` is a tree: `require((isA(msg.sender) || msg.sender == owner) && !paused)` has two `conjuncts`, the first with two `alternatives`. Both lists are empty for a single test.
- A term that is not about the caller has the kind of its flag, `pause` or `initializer`, else `condition`.
- A compound term, and a compound guard, carry no partial fact: `compared_against`, `checked_by`, `role`, `role_implied_by` and `account` are `null`, the parts carry them. Its `guard_kind` and `negated` come from the part that decides it: in a conjunction the first part that restricts, else the first that denies; in a disjunction the first part about the caller.
- `negated` is the polarity of the requirement, not of the syntax: `true` when the call must be false to pass. `if (!hasRole(r, msg.sender)) revert` has `negated: false` (a role is required); `require(!isBlacklisted(msg.sender))` has `negated: true`. A negated caller-dependent predicate is a deny-list: it is exported as a guard with `negated: true` and never restricts access. For a term that is not about the caller, `negated` is the same polarity: `!paused` is `{guard_kind: "pause", condition_text: "paused", negated: true}`.
- `restricts` is `true` when the guard holds on every path (`scope: unconditional`) and every way to pass needs a caller test that must hold. A disjunction one of whose branches lets any caller through, as `require(msg.sender == admin || open)`, is still exported, with `open` as an alternative of kind `condition`, and has `restricts: false`. A deny-list has `restricts: false` too.
- A guard whose condition also tests the caller in a way the analysis does not read (a comparison of `uint160(msg.sender)`, a ternary) has tag `AMBIGUOUS` and empty `alternatives` and `conjuncts`: `condition_text` holds the whole condition. When the condition is compound, the guard's own `compared_against`, `checked_by`, `role`, `role_implied_by` and `account` are `null` too; `guard_kind`, `negated` and `restricts` still reflect what was read.

### Spans

A direct fact points at its statement; a read or revert, which the control-flow graph records per block, at its block. A fact with `via` points at the call in the function that leads to it. A guard found only by a modifier name points at the function.

## Known gaps

- `dispatch_target` is computed for internal calls only; an external call keeps the interface declaration.
- A condition that tests the caller only in a way the analysis does not read yields no access guard.

## Out of v1

Proxies, interface to implementation resolution, Yul semantics, branch-sensitive dominance, a partial compile of the sources that excludes failing files and their importers, control-flow blocks, transaction sequences, paths and storage slots.
