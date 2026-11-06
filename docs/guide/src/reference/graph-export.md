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

Edge ids are `<kind>:<from>#<hash>.<n>`. The hash (16 hex digits) covers the record without `span`, `tag`, `rule` and `id`, so it never depends on positions: moving unrelated code keeps every id. Fields added to a kind after schema 1.0 (on `calls`: `dispatch_candidates`, `unresolved_reason`, `target_source.assigned_at` and `in_try`; on `reads` and `writes`: `keys`, `keys_reason` and `key_sites`) are left out of the hash too, so a record keeps the id 1.0 gave it. The `from` of a `contract_link` is its `from_contract`. A `try_catch` hashes only `function`, `in_context`, `guarded_call` and `via`, so editing a catch clause keeps its id. A `value_transfer` hashes `function`, `in_context`, `asset_kind`, `operation`, its five expressions, `via_library` and `via`; a `balance_read` the same without `operation`, `from_expr`, `to_expr` and `amount_expr` and with `owner_expr`. `<n>` numbers, from `0`, the edges of `<from>` that share the hash, ordered by span (file, then line and column); it is always present, so a first occurrence keeps `.0` when another one is added later. Equal facts, position included, collapse into one line.

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
| `EXTRACTED` | Read from the syntax: declarations, reverts, code facts, guards found in code on every path, storage-parameter accesses, try statements, ether sent by `.transfer`, `.send` or `{value: ...}`, `.balance` reads. |
| `TYPE_RESOLVED` | Resolved through solc declaration ids and types: call targets, emitted events, state variables of reads and writes, linearization, token and library value transfers and balance reads. |
| `INFERRED` | A heuristic: guards from a modifier name, guards on some paths only (`scope: conditional`), events matched by name where solc gave no id, calls resolved by name, linearization without solc data, an external call with exactly one in-scope implementation. |
| `AMBIGUOUS` | Unresolved: the edge keeps its text and `to`, `event` or `state_var` is `null`. A modifier without code is `AMBIGUOUS` too, and so are dispatch candidates and contract links when several in-scope contracts may answer one call, and a try statement whose clauses cannot be told apart (`reason`), and a value transfer or balance read whose asset is not decided (`asset_candidates`, `reason`). |

### Rules

| Rule | Pass |
| --- | --- |
| `parse` | The solc AST model: every node. |
| `linearization` | C3 linearization and name lookup over it: `inherits`, `exposes`, `uses_modifier`. |
| `cfg` | The function's control-flow graph, modifiers inlined: direct `calls`, `reads`, `writes`, `emits`, `reverts`, `opaque_assembly`, `try_catch`. |
| `effects` | Effects of the functions it calls, which run on its storage: facts with a non-empty `via`. |
| `guards` | The guard walk over the function, its modifiers and helpers. |
| `code_facts` | Builtins and unchecked blocks, modifiers included. |
| `dispatch` | Which in-scope contracts may answer external calls: `contract_link`. |
| `value` | The recognizer table over the function's calls and `.balance` reads, modifiers included: `value_transfer`, `balance_read`. |

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
| `calls` | `from`, `to` (`dispatch_target`, else `static_target`, else `null`), `target_text`, `call_kind`, `static_target` (the declaration the code names), `dispatch_target` (what runs in this context: the virtual override, the `super` target), `options` (`{value, gas, salt}` as written), `target_source`, `result_used` (low-level calls and `send` only), `via`. External calls to a contract (`external`, `external_self`) also carry `dispatch_candidates` and, when that list is empty, `unresolved_reason`. See [Dispatch candidates](#dispatch-candidates). The call a try statement guards carries `in_try`, the id of its `try_catch`. |
| `try_catch` | `function`, `in_context`, `guarded_call` (the id of the `calls` record it guards, `null` when unknown), `success_span`, `catch_clauses`, `reason`, `via`. See [Try statements](#try-statements). |
| `value_transfer` | `function`, `in_context`, `asset_kind`, `asset_candidates`, `operation`, `direction`, `token_expr`, `from_expr`, `to_expr`, `amount_expr`, `id_expr`, `call_id` (the `calls` record that makes it), `via_library`, `reason`, `via`. See [Value transfers and balance reads](#value-transfers-and-balance-reads). |
| `balance_read` | `function`, `in_context`, `asset_kind`, `asset_candidates`, `token_expr`, `owner_expr`, `id_expr`, `via_library`, `reason`, `via`. |
| `contract_link` | `from_contract`, `to_contract`, `via_state_vars` (ids of the state variables holding the called address), `call_ids` (the `calls` records it aggregates), `basis`. See [Contract links](#contract-links). |
| `reads`, `writes` | `state_var`, `path` (`balances[]`, `s.x`), `param` (the storage parameter it goes through, then `state_var` is `null`), `via`, `keys`, `keys_reason`, `key_sites`. See [Mapping keys](#mapping-keys). |
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

On `target_source`, a `state_var`, `immutable` or `constant` also carries `assigned_at`: every place the variable gets its value, `{function, span, expr_text, kind}`, sorted by position. `function` is `null` for the declaration's initial value; `kind` is `constructor_param` (a constructor parameter), `initializer` (a function named `initialize` or with an `initializer`/`reinitializer` modifier), `setter` (any other function), `literal` (a constant expression, in a constructor or the declaration) or `other`. An empty list means the code never assigns it.

So `require(msg.sender == owner)` compares against `{kind: "state_var", name: "owner", id: ...}` and `require(msg.sender == to)` against `{kind: "param", index: 0, name: "to"}`.

### Guard conditions

An access guard's condition is split into terms. A term is `{guard_kind, condition_text, negated, compared_against, checked_by, role, role_implied_by, account, alternatives, conjuncts}`, `condition_text` being its part of the condition:

- `alternatives` lists every disjunct of an `||`, `conjuncts` every conjunct of an `&&`, in source order; a chain of the same operator is one list. Each part is itself a term, so a mix of `&&` and `||` is a tree: `require((isA(msg.sender) || msg.sender == owner) && !paused)` has two `conjuncts`, the first with two `alternatives`. Both lists are empty for a single test.
- A term that is not about the caller has the kind of its flag, `pause` or `initializer`, else `condition`.
- A compound term, and a compound guard, carry no partial fact: `compared_against`, `checked_by`, `role`, `role_implied_by` and `account` are `null`, the parts carry them. Its `guard_kind` and `negated` come from the part that decides it: in a conjunction the first part that restricts, else the first that denies; in a disjunction the first part about the caller.
- `negated` is the polarity of the requirement, not of the syntax: `true` when the call must be false to pass. `if (!hasRole(r, msg.sender)) revert` has `negated: false` (a role is required); `require(!isBlacklisted(msg.sender))` has `negated: true`. A negated caller-dependent predicate is a deny-list: it is exported as a guard with `negated: true` and never restricts access. For a term that is not about the caller, `negated` is the same polarity: `!paused` is `{guard_kind: "pause", condition_text: "paused", negated: true}`.
- `restricts` is `true` when the guard holds on every path (`scope: unconditional`) and every way to pass needs a caller test that must hold. A disjunction one of whose branches lets any caller through, as `require(msg.sender == admin || open)`, is still exported, with `open` as an alternative of kind `condition`, and has `restricts: false`. A deny-list has `restricts: false` too.
- A guard whose condition also tests the caller in a way the analysis does not read (a comparison of `uint160(msg.sender)`, a ternary) has tag `AMBIGUOUS` and empty `alternatives` and `conjuncts`: `condition_text` holds the whole condition. When the condition is compound, the guard's own `compared_against`, `checked_by`, `role`, `role_implied_by` and `account` are `null` too; `guard_kind`, `negated` and `restricts` still reflect what was read.

### Dispatch candidates

`dispatch_target` keeps its meaning: it is `null` for an external call. `dispatch_candidates` lists the deployable project contracts (`deployable: true`, not dependencies) that may answer the call, each `{function, contract, basis, tag}`:

| `basis` | The candidate |
| --- | --- |
| `inherits_interface` | inherits the contract the call names, directly or transitively, and implements the function (or a getter with its selector). |
| `selector_match` | exposes the selector without inheriting it. Only listed when no contract inherits the interface. |
| `self` | `this.f()`: the calling contract, running the implementation it sees. |

One candidate is `INFERRED`; several are all `AMBIGUOUS`. Nothing picks one of them. With none, `unresolved_reason` says why: `out_of_scope_interface` (the named contract is a dependency and nothing in the sources implements it), `no_implementation` (it is in the sources and nothing implements it) or `dynamic_address` (the call names no declaration, as a call through a function pointer).

```json
{"kind": "calls", "from": "evm:src/Router.sol:Router.run(uint256)", "target_text": "vault.deposit", "call_kind": "external",
 "static_target": "evm:src/Interfaces.sol:IVault.deposit(uint256)", "dispatch_target": null,
 "dispatch_candidates": [{"function": "evm:src/Impls.sol:Vault.deposit(uint256)", "contract": "evm:src/Impls.sol:Vault", "basis": "inherits_interface", "tag": "INFERRED"}],
 "target_source": {"kind": "immutable", "name": "vault", "id": "evm:src/Router.sol:Router.vault",
   "assigned_at": [{"function": "evm:src/Router.sol:Router.constructor(IVault,IHook,IOracle)", "expr_text": "v", "kind": "constructor_param", "span": {"file": "src/Router.sol", "start_line": 15, "start_col": 9, "end_line": 15, "end_col": 18}}]},
 "tag": "TYPE_RESOLVED", "...": "..."}
{"kind": "calls", "target_text": "orphan.ping", "dispatch_candidates": [], "unresolved_reason": "no_implementation", "...": "..."}
```

### Contract links

One `contract_link` per ordered pair of contracts: code declared in `from_contract` makes external calls that `to_contract` may answer. It aggregates the candidates of those calls: `basis` is the strongest among them (`self`, then `inherits_interface`, then `selector_match`), and the tag is `INFERRED` when some call has `to_contract` as its only candidate, else `AMBIGUOUS`. A `this.f()` call links a contract to itself. The span is the first call's.

```json
{"kind": "contract_link", "id": "contract_link:evm:src/Router.sol:Router#70ab8c107d3eed65.0",
 "from_contract": "evm:src/Router.sol:Router", "to_contract": "evm:src/Impls.sol:Vault",
 "via_state_vars": ["evm:src/Router.sol:Router.vault"],
 "call_ids": ["calls:evm:src/Router.sol:Router.run(uint256)#cfe0e483bcc9b7a1.0"],
 "basis": "inherits_interface", "tag": "INFERRED", "rule": "dispatch",
 "span": {"file": "src/Router.sol", "start_line": 30, "start_col": 9, "end_line": 30, "end_col": 25}}
```

### Mapping keys

`path` drops the keys (`balances[]`); `keys` says what each index level reads, outermost first, as `{text, kind, ref}`. `text` is the index as the source writes it. `kind`:

| `kind` | The index is | `ref` |
| --- | --- | --- |
| `msg_sender` | `msg.sender`: a slot per caller. | `null` |
| `parameter` | a parameter of the code making the access. | the function, or the modifier for modifier code. |
| `state_var` | a state variable, constants and immutables included. | the state variable. |
| `local` | a local variable or named return. | `null` |
| `literal` | a literal. | `null` |
| `this` | `this`, casts such as `address(this)` included. | `null` |
| `expression` | anything else: a call, arithmetic, a member. | `null` |

A text that names different declarations in the same code (a local shadowing a parameter) is an `expression`: nothing picks one of them. The parameters of a callee that returns a storage pointer key what it returns, as in `_position(key).amount`, which writes `positions[id].amount` keyed by `_position`'s `id`.

Each record of the function's own code lists the distinct key sets it occurs with in `key_sites`, `{keys, span}` sorted by span, then key text; records are never split by key, so their count and ids are those of schema 1.0. `keys` is the key set every site shares; a path without an index has `keys: []`. Reads are recorded per block, so a block reading `balances[from]` and `balances[to]` is one record whose sites differ: then `keys` is `null` and `keys_reason` is `several`. A record with `via` happens in a callee, whose summary keeps no keys: `keys` is `null`, `keys_reason` is `transitive`, `key_sites` is empty, and `via` leads to the callee's own record, which has them.

```json
{"kind": "writes", "function": "evm:src/Keys.sol:Keys.credit(address,uint256)", "path": "balances[]", "via": [],
 "keys": [{"text": "account", "kind": "parameter", "ref": "evm:src/Keys.sol:Keys.credit(address,uint256)"}], "keys_reason": null,
 "key_sites": [{"keys": [{"text": "account", "kind": "parameter", "ref": "evm:src/Keys.sol:Keys.credit(address,uint256)"}],
   "span": {"file": "src/Keys.sol", "start_line": 26, "start_col": 9, "end_line": 26, "end_col": 35}}], "...": "..."}
{"kind": "reads", "function": "evm:src/Keys.sol:Keys.move(address,address,uint256)", "path": "balances[]", "via": [],
 "keys": null, "keys_reason": "several",
 "key_sites": [{"keys": [{"text": "from", "kind": "parameter", "ref": "evm:src/Keys.sol:Keys.move(address,address,uint256)"}], "span": {"...": "..."}},
   {"keys": [{"text": "to", "kind": "parameter", "ref": "evm:src/Keys.sol:Keys.move(address,address,uint256)"}], "span": {"...": "..."}}], "...": "..."}
{"kind": "writes", "function": "evm:src/Keys.sol:Keys.creditLater(address,uint256)", "path": "balances[]",
 "via": ["evm:src/Keys.sol:Keys._credit(address,uint256)"], "keys": null, "keys_reason": "transitive", "key_sites": [], "...": "..."}
```

### Try statements

One `try_catch` per try statement the function runs, its span the whole statement. `success_span` covers the success clause, from `returns` (or its block) to the closing brace. Each of `catch_clauses`, in source order, is `{kind, span, empty, effects}`:

| `kind` | The clause |
| --- | --- |
| `Error` | `catch Error(string memory reason)`: a revert with a reason string. |
| `Panic` | `catch Panic(uint256 code)`: a failed assertion, overflow, division by zero. |
| `bytes` | `catch (bytes memory data)`: any failure, with its revert data. |
| `all` | `catch { ... }`: any failure, without its data. |

The kind comes from the clause's declaration, so a `bytes` catch and a bare one differ even where the control-flow graph names both alike. `empty` is `true` when the clause has no statement: the failure is swallowed and execution goes on after the try. `effects` is what the clause's own code does: `writes` (`{path, state_var, span}`, `path` keeping its keys), `calls` (`{text, span}`), `emits` (`{event, span}`, the event's name) and `reverts` (some path through the clause reverts). The effects of a function the clause calls are that function's records; code after the try statement belongs to no clause. The guarded `calls` record carries `in_try`, so the call reads as "may fail without reverting the caller".

A try statement whose guarded call or clauses the model cannot read is `AMBIGUOUS` with `reason`: `statement_not_found` (no try statement has the call, so `catch_clauses` is empty) or `unknown_catch_kind` (a clause names an error other than `Error` or `Panic`; its `kind` is `null`).

```json
{"kind": "try_catch", "id": "try_catch:evm:src/TryCatch.sol:TryCatch.swallow()#6c18f36ba0abe966.0",
 "function": "evm:src/TryCatch.sol:TryCatch.swallow()", "in_context": "evm:src/TryCatch.sol:TryCatch",
 "guarded_call": "calls:evm:src/TryCatch.sol:TryCatch.swallow()#eff18cd9268cc749.0", "via": [],
 "success_span": {"file": "src/TryCatch.sol", "start_line": 46, "start_col": 38, "end_line": 46, "end_col": 40},
 "catch_clauses": [{"kind": "all", "empty": true, "effects": {"writes": [], "calls": [], "emits": [], "reverts": false},
   "span": {"file": "src/TryCatch.sol", "start_line": 46, "start_col": 41, "end_line": 46, "end_col": 49}}],
 "reason": null, "tag": "EXTRACTED", "rule": "cfg",
 "span": {"file": "src/TryCatch.sol", "start_line": 46, "start_col": 9, "end_line": 46, "end_col": 49}}
{"kind": "calls", "target_text": "this.step", "call_kind": "external_self",
 "in_try": "try_catch:evm:src/TryCatch.sol:TryCatch.swallow()#6c18f36ba0abe966.0", "...": "..."}
{"kind": "try_catch", "function": "evm:src/TryCatch.sol:TryCatch.clauses(uint256)",
 "catch_clauses": [
   {"kind": "Error", "empty": false, "effects": {"emits": [{"event": "Rejected", "span": {"...": "..."}}], "writes": [], "calls": [], "reverts": false}, "span": {"...": "..."}},
   {"kind": "Panic", "empty": false, "effects": {"writes": [{"path": "panics[code]", "state_var": "evm:src/TryCatch.sol:TryCatch.panics", "span": {"...": "..."}}],
     "calls": [], "emits": [], "reverts": false}, "span": {"...": "..."}},
   {"kind": "bytes", "empty": false, "effects": {"writes": [{"path": "lastData", "state_var": "evm:src/TryCatch.sol:TryCatch.lastData", "span": {"...": "..."}}],
     "calls": [{"text": "pinger.ping", "span": {"...": "..."}}], "emits": [], "reverts": false}, "span": {"...": "..."}}],
 "...": "..."}
```

### Value transfers and balance reads

One `value_transfer` per call that moves ether or tokens, one `balance_read` per balance the code reads, from a single recognizer table (`crates/ilold-core/src/relations/value.rs`). A call or member access the table does not list yields no record: a call named `transfer` whose declaration has another signature, a low-level call without `{value: ...}`, a transfer written in assembly.

| Form | Recognized by | `asset_kind` | `operation` | Fields |
| --- | --- | --- | --- | --- |
| `to.transfer(x)`, `to.send(x)` | call kind | `native` | `transfer`, `send` | `to`, `amount` |
| `to.call{value: x}(...)`, `c.f{value: x}(...)` | a `value` option on a low-level or external call | `native` | `call` | `to`, `amount` |
| `new C{value: x}(...)` | a `value` option on a creation | `native` | `create` | `amount` |
| `a.balance` | the `balance` builtin | `native` | (read) | `owner` |
| `transfer(address,uint256)` | selector `a9059cbb` | `erc20` | `transfer` | `token`, `to`, `amount` |
| `transferFrom(address,address,uint256)` | `23b872dd` | `erc20` or `erc721` | `transfer_from` | `token`, `from`, `to`, `amount` (ERC-20) or `id` (ERC-721) |
| `safeTransferFrom(address,address,uint256)`, `(…,bytes)` | `42842e0e`, `b88d4fde` | `erc721` | `safe_transfer_from` | `token`, `from`, `to`, `id` |
| `safeTransferFrom(address,address,uint256,uint256,bytes)`, `safeBatchTransferFrom(…)` | `f242432a`, `2eb2c2d6` | `erc1155` | `safe_transfer_from` | `token`, `from`, `to`, `id`, `amount` |
| `transfer(address,uint256,uint256)`, `transferFrom(address,address,uint256,uint256)` | `095bcdb6`, `fe99049a` | `erc6909` | `transfer`, `transfer_from` | `token`, `from`, `to`, `id`, `amount` |
| `mint(address,uint256)` | `40c10f19` | `erc20` or `erc721` | `mint` | `token`, `to`, `amount` or `id` |
| `mint(address,uint256,uint256,bytes)`, `mint(address,uint256,uint256)` | `731133e9`, `156e29f6` | `erc1155`, `erc6909` | `mint` | `token`, `to`, `id`, `amount` |
| `burn(uint256)`, `burn(address,uint256)`, `burnFrom(address,uint256)` | `42966c68`, `9dc29fac`, `79cc6790` | `erc20` (`burn(uint256)` also `erc721`) | `burn` | `token`, `from`, `amount` or `id` |
| `burn(address,uint256,uint256)` | `f5298aca` | `erc1155` or `erc6909` | `burn` | `token`, `from`, `id`, `amount` |
| `balanceOf(address)`, `balanceOf(address,uint256)` | `70a08231`, `00fdd58e` | `erc20`/`erc721`, `erc1155`/`erc6909` | (read) | `token`, `owner`, `id` |
| OpenZeppelin `SafeERC20.safeTransfer`, `safeTransferFrom`, `trySafeTransfer`, `trySafeTransferFrom` | the library function the call resolves to | `erc20` | `transfer`, `transfer_from` | `token`, `from`, `to`, `amount` |
| OpenZeppelin `Address.sendValue`, `functionCallWithValue` | library function | `native` | `call` | `to`, `amount` |
| Solady/Solmate `SafeTransferLib.safeTransferETH`, `forceSafeTransferETH`, `trySafeTransferETH` | library function | `native` | `call` | `to`, `amount` |
| `SafeTransferLib.safeTransfer`, `safeTransferFrom`, `balanceOf` | library function | `erc20` | `transfer`, `transfer_from`, (read) | `token`, `from`, `to`, `amount`, `owner` |
| Uniswap v4 `CurrencyLibrary.transfer`, `balanceOf`, `balanceOfSelf` | library function | `unknown`: `native` or `erc20` | `transfer`, (read) | `token`, `to`, `amount`, `owner` |
| `CurrencySettler.settle(currency, manager, payer, amount, burn)`, `take(currency, manager, recipient, amount, claims)` | library function | `erc6909` when the flag is literally `true`; otherwise `unknown` | `settle`, `take` | `token`, `from`, `to`, `amount` |

A token call is matched by the selector of the declaration solc resolves, then by the standard its declaring interface follows, itself or through what it inherits: `erc20` exposes `transfer(address,uint256)` or `allowance(address,address)`, `erc721` exposes `ownerOf(uint256)`, `erc1155` exposes `safeTransferFrom(address,address,uint256,uint256,bytes)` or `safeBatchTransferFrom`, `erc6909` exposes `transfer(address,uint256,uint256)` or `transferFrom(address,address,uint256,uint256)`. When that does not decide, the record is `AMBIGUOUS`, `asset_kind` is `unknown`, `asset_candidates` lists the kinds it may be, and a field the candidates read from different arguments is `null`. `reason` says why:

| `reason` | |
| --- | --- |
| `no_standard` | The declaring interface follows none of the standards, as `interface IBurnable { function burn(uint256) external; }`. |
| `several_standards` | It follows several. |
| `unresolved_declaration` | The call names no declaration; only its name and argument count match. |
| `currency` | A `Currency` is ether or an ERC-20 token at run time. |
| `claims_flag` | `settle` and `take` move ERC-6909 claims when their last argument is `true`, and it is not a literal. |

The `*_expr` fields are the source text of the argument. A `null` `from_expr` is the contract running the code (the sender of a `transfer`, a `send`, a `transfer(to, x)`); for a mint, nobody. A `null` `to_expr` is a burn's sink or a contract being created. `direction` is `in` when `to_expr` names the running contract (`this`, `address(this)`), `out` when the sender is the running contract, `other` when the value moves between two other accounts (`transferFrom(a, b, x)`, `take` to a third party). Literal units are lost: `1 ether` reads `1`.

A library helper is recorded at the call site in the function that calls it, `via_library` naming the library function, since the library's own code is out of scope and often moves the value in assembly. The library function's own code is analysed too and yields its own records, with the library function as `function`. A transfer inside modifier code names the modifier in `via`.

```json
{"kind": "value_transfer", "id": "value_transfer:evm:src/Value.sol:Payments.sendEth(address,uint256)#1fc41cabda8eb951.0",
 "function": "evm:src/Value.sol:Payments.sendEth(address,uint256)", "in_context": "evm:src/Value.sol:Payments",
 "asset_kind": "native", "asset_candidates": [], "operation": "call", "direction": "out",
 "token_expr": null, "from_expr": null, "to_expr": "to", "amount_expr": "amount", "id_expr": null,
 "call_id": "calls:evm:src/Value.sol:Payments.sendEth(address,uint256)#036146ab7f8650e7.0",
 "via_library": "evm:lib/vendor/Libraries.sol:Address.sendValue(address,uint256)", "reason": null, "via": [],
 "tag": "TYPE_RESOLVED", "rule": "value",
 "span": {"file": "src/Value.sol", "start_line": 88, "start_col": 9, "end_line": 88, "end_col": 38}}
{"kind": "value_transfer", "function": "evm:src/Value.sol:Payments.buy(uint256)", "asset_kind": "native", "operation": "transfer",
 "to_expr": "payable(msg.sender)", "amount_expr": "msg.value - price", "via_library": null,
 "via": ["evm:src/Value.sol:Payments.refundExcess"], "tag": "EXTRACTED", "...": "..."}
{"kind": "value_transfer", "function": "evm:src/Value.sol:Payments.burnShares(uint256)", "asset_kind": "unknown",
 "asset_candidates": ["erc20", "erc721"], "operation": "burn", "direction": "out", "token_expr": "burnable",
 "amount_expr": null, "id_expr": null, "reason": "no_standard", "tag": "AMBIGUOUS", "...": "..."}
{"kind": "value_transfer", "function": "evm:src/Value.sol:Payments.settleQuote(uint256,bool)", "asset_kind": "unknown",
 "asset_candidates": ["native", "erc20", "erc6909"], "operation": "take", "direction": "other",
 "token_expr": "quote", "from_expr": "manager", "to_expr": "msg.sender", "amount_expr": "amount",
 "via_library": "evm:lib/vendor/Libraries.sol:CurrencySettler.take(Currency,IPoolManager,address,uint256,bool)",
 "reason": "claims_flag", "tag": "AMBIGUOUS", "...": "..."}
{"kind": "balance_read", "function": "evm:src/Value.sol:Payments.balances(address,uint256)", "asset_kind": "erc6909",
 "asset_candidates": [], "token_expr": "manager", "owner_expr": "who", "id_expr": "id", "via_library": null,
 "reason": null, "via": [], "tag": "TYPE_RESOLVED", "rule": "value", "...": "..."}
```

### Spans

A direct fact points at its statement; a read or revert, which the control-flow graph records per block, at its block. A fact with `via` points at the call in the function that leads to it. A guard found only by a modifier name points at the function.

## Known gaps

- `dispatch_target` is computed for internal calls only; an external call keeps the interface declaration and lists its in-scope implementations in `dispatch_candidates` instead.
- Dispatch candidates do not follow the value of the address: a call through `hook` lists every implementation of `IHook`, whatever `assigned_at` sets it to.
- A condition that tests the caller only in a way the analysis does not read yields no access guard.
- Keys are read from the index's text in the code making the access, its modifiers and the callees returning a storage pointer it calls; a key that reaches the path another way, or a text naming two declarations there, is an `expression`. So is a local declared in a tuple, `(address a, uint256 b) = ...`: the model keeps no declaration ids for its parts.
- An access made in a callee carries no keys (`keys_reason: "transitive"`); its keys are on the callee's own record.
- Catch clause effects list the clause's own code only, and name calls and events by their text, not by record id.
- Value is recognized only from the table above. A transfer made in a function the code calls is that function's record, reached through the `calls` record; it is not repeated on the caller.
- `direction` reads the source text: an address that equals the running contract only at run time reads as another account.
- The model drops a literal's unit, so `1 ether` and `1 days` read `1`.

## Out of v1

Proxies, interface to implementation resolution by the deployed address, Yul semantics, branch-sensitive dominance, a partial compile of the sources that excludes failing files and their importers, control-flow blocks, transaction sequences, paths and storage slots.
