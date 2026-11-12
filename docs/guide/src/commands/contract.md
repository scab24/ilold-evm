# Contract Commands

Contract commands inspect the structure of the loaded contracts without modifying the session.

## functions

`f` or `functions`

Lists the callable functions in the active contract with their access level and tags.

```
ilold[Staking]> f

  [P] deposit       writes state  {pause} {reentrancy}
  [P] withdraw      writes state, external calls  {reentrancy}
  [P] claimRewards  writes state, external calls
  [P] getStakeInfo  view
  [R] pause         writes state  {caller}
  [R] unpause       writes state  {caller}
```

Tags indicate `writes state`, `external calls`, or `view` (read-only, no external calls).

The badges in braces are the function's guards, read from code: revert checks on the entry path, in the body, in an inlined modifier or in a helper or library function they call. Access guards restrict who may call and make the function `[R]`:

- `{caller}`: the caller compared with an address (`msg.sender == owner`, `owner() != _msgSender()`).
- `{role}`: the caller passed to a check or used as a key (`hasRole(role, sender)`, `acl.isPoolAdmin(msg.sender)`, `authorized[msg.sender]`).
- `{tx.origin}`: a check on `tx.origin`.

A check restricts only when its condition must hold for the caller to get through. `require(msg.sender != owner)`, `if (msg.sender == owner) revert` and `require(!blocked[msg.sender])` let everyone but some callers in: they are reported as deny-lists (`negated`) and never make the function `[R]`. `require(msg.sender == admin || open)` is reported too, but never restricts either, because anyone passes while `open` is true. A call that takes the caller is a role check only when the called function is `view` or `pure`; `require(token.transferFrom(msg.sender, ...))` is not.

A check inside a loop, inside an `if`/`else` branch that does not simply revert, or after a branch, loop or assembly block that may return early holds on some paths only. It is still reported, with scope `Conditional` and a badge ending in `*` (`{caller*}`), but never makes the function `[R]`. Helpers are followed 12 calls deep; a function whose guard walk was cut there is marked `guards_incomplete`.

Each access guard records what the caller is compared with (`compared_against`): a state variable, immutable, constant, parameter, literal, `msg.sender` or another expression, so `require(msg.sender == owner)` and `require(msg.sender == to)` differ. A role check records the role argument (`role`), or the predicate that names the role when no argument does (`role_implied_by`, as in `isPoolAdmin(msg.sender)`), and what it consults (`checked_by`, the called function or the mapping). Every access guard names the account it checks (`account`: `msg.sender` or `tx.origin`, through modifier and helper parameters and `_msgSender()`). A disjunction such as `require(isRiskAdmin(msg.sender) || isPoolAdmin(msg.sender))` lists each disjunct in `alternatives`, and a conjunction such as `require(msg.sender == owner && !paused)` each conjunct in `conjuncts`.

State guards do not restrict the caller and leave the function `[P]`: `{pause}`, `{reentrancy}` and `{initializer}`. A modifier whose name sounds like access control but whose check reads a flag is not an access guard. Only a modifier with no code to read falls back to its name; its badge ends in `?` (`{access?}`).

## funcs-all

`fa` or `funcs-all`

Lists all accessible functions including those inherited from parent contracts.

```
ilold[Staking]> fa

  [P] deposit       writes state  {pause} {reentrancy}
  [P] withdraw      writes state, external calls  {reentrancy}
  [P] claimRewards  writes state, external calls
  [P] getStakeInfo  view
  [R] pause         writes state  {caller}
  [R] unpause       writes state  {caller}

  inherited:
  [P] owner              from Ownable
  [R] transferOwnership  from Ownable  {caller}
```

Inherited functions are listed separately with their origin contract. Their access and guards are those of the active contract: modifiers and virtual helpers resolve to the overrides it sees.

## vars

`v` or `vars`

Lists the state variables of the active contract with their type and mutability tag.

```
ilold[Staking]> v

  mutable   balances      mapping(address => uint256)
  mutable   totalStaked   uint256
  mutable   rewardDebt    mapping(address => uint256)
  mutable   paused        bool
  const     MIN_STAKE     uint256
  immutable rewardToken   address
```

Tags are `mutable`, `const`, or `immutable`. A mapping or array also lists the kinds of key the contract's functions index it with, such as `keys: msg_sender, parameter`.

## vars-all

`va` or `vars-all`

Lists all accessible state variables including inherited ones.

```
ilold[Staking]> va

  mutable   balances      mapping(address => uint256)
  mutable   totalStaked   uint256
  mutable   rewardDebt    mapping(address => uint256)
  mutable   paused        bool
  const     MIN_STAKE     uint256
  immutable rewardToken   address

  inherited:
  mutable   _owner        address  from Ownable
```

## contracts

`ct` or `contracts`, `ct all`

Lists the project contracts with their type, function count, state variable count, and inheritance. Contracts that share a name are shown by qualified id (`src/core/Token.sol:Token`); `use` accepts either form and reports an ambiguous bare name. Dependency contracts (libraries and vendored code imported by the sources) are loaded for resolution but hidden; `ct all` lists them too, marked `(dependency)`.

```
ilold[Staking]> ct

  [C] Staking   6 functions, 6 state vars, inherits Ownable, ReentrancyGuard  ← current
  [C] Ownable   3 functions, 1 state vars
  [A] ReentrancyGuard  0 functions, 1 state vars
  [I] IERC20    6 functions, 0 state vars
```

Type badges: `[C]` contract, `[I]` interface, `[L]` library, `[A]` abstract.

## deps

`deps [contract]`

Lists what a contract depends on — the current contract, or the named one. Each
dependency is tagged with the kind of relationship:

- **inherits** — the contract extends it (`is`).
- **calls** — a function calls into it (resolved to the real target contract).
- **holds** — it has a state variable of that contract/interface type.
- **dispatches** — it calls an interface that this deployable contract implements, so the call may run here. The tag shows the basis (`inherits_interface`, `selector_match`, `self` for `this.f()`), the calls it covers and the state variable holding the address; `AMBIGUOUS` when other contracts may answer the same call.

```
ilold[Vault]> deps

  Vault → depends on
    IPool     calls×2 holds
    SafeMath  calls×2
```

Here `Vault` calls into `IPool` twice and keeps an `IPool` state variable
(`holds`), and calls the `SafeMath` library through a `using` directive. A call through an interface also lists its implementations:

```
ilold[Router]> deps

  Router → depends on
    HookA      dispatches inherits_interface×1 via hook AMBIGUOUS
    HookB      dispatches inherits_interface×1 via hook AMBIGUOUS
    IHook      calls×1 holds
    IOracle    calls×1 holds
    IOrphan    calls×1 holds
    IRegistry  calls×1 holds
    IVault     calls×1 holds
    Registry   dispatches selector_match×1 via registry
    Router     dispatches self×1
    Vault      dispatches inherits_interface×1 via vault
```

## usedby

`usedby [contract]`

The reverse direction — which contracts depend on this one. This is the blast
radius: if you change the contract, these are the contracts that could be
affected.

```
ilold[IPool]> usedby IPool

  IPool ← used by (blast radius)
    LendingPool  inherits
    Vault        calls×2 holds
```

The same relationships are available project-wide on the CLI with
`ilold analyze <project> --deps`, which prints every contract grouped into
topological reading order (dependency-free contracts first), and as an
interactive graph on the web canvas.

## links

`links [contract | --all]`

Lists the calls between a contract — the current one, or the named one — and
other contracts, function to function, as the contract runs its code: inherited
functions and base constructors are included, so a call an abstract base makes
is listed under every contract that inherits it. Only deployed contracts call
others on their own: an abstract contract or a library is never listed under
`called by`, though `links` on it still lists its own calls.

- **calls** — the calls it makes, grouped by the contract that may answer. A
  call through an interface several contracts implement is one row, listed under
  each of them, with `N possible` and `AMBIGUOUS`. Library calls are listed
  under the library and `this.f()` calls under the contract itself. Rows other
  than plain calls say `read` (a view or pure function or a getter), `library`
  or `self`, `via` names the state variable holding the called address,
  `moves` the asset and amount the call sends and `in try` a call a try
  statement guards.
- **called by** — the calls into it, grouped by the contract running the
  calling code.
- **Unresolved** — calls nothing in the project answers, with the reason
  (`out_of_scope_interface`, `no_implementation`, `dynamic_address`).
- **Addresses set at** — where the state variables named in `via` get their
  value.

```
ilold[Curve]> links

  Curve → calls
    DenA  1 call
      buy → 2 possible: DenA::deliver, DenB::deliver  line 36  via CurveBase::den  AMBIGUOUS
    DenB  1 call
      buy → 2 possible: DenA::deliver, DenB::deliver  line 36  via CurveBase::den  AMBIGUOUS
    Hook  2 calls
      CurveBase::_graduate → Hook::migrate  line 27  via CurveBase::hook  moves native address(this).balance
      CurveBase::_graduate → Hook::fee  read  line 27  via CurveBase::hook
    Payout  1 call
      buy → Payout::pay  library  line 41
    Token  2 calls
      buy → Token::mint  line 35  via CurveBase::token  moves erc20 token msg.value
      Payout::pay → Token::transfer  line 8  moves erc20 token amount

  Curve ← called by
    Router  2 calls
      Router::buyWith → Curve::buy  line 14  via Router::curve  moves native msg.value
      Router::quote → CurveBase::price  read  line 18  via Router::curve

  Addresses set at
      CurveBase::den = den_  constructor_param  CurveBase::constructor  line 15
      CurveBase::hook = hook_  constructor_param  CurveBase::constructor  line 13
      CurveBase::token = token_  constructor_param  CurveBase::constructor  line 14
```

Functions of the contract itself are shown by name; inherited ones keep their
declaring contract (`CurveBase::_graduate`). The code of a project library the
contract calls runs inside the contract, so its calls are listed as the
contract's own (`Payout::pay`); a dependency library stays a single `library`
row.

`links --all` lists the whole project instead: every pair of a deployed
contract and a contract answering its calls, with its calls, reads and library
calls (a call with several candidates counted once under each), how many go
through an interface the target inherits, the value moved and `AMBIGUOUS`; then
the unresolved calls per interface, and the calls and reads to and from other
contracts of each deployed contract. The project map draws the same pairs.

```
ilold[Curve]> links --all

  Project connections  6 pair(s)
    Curve → DenA  1 call  1 via interface  AMBIGUOUS
    Curve → DenB  1 call  1 via interface  AMBIGUOUS
    Curve → Hook  1 call, 1 read  2 via interface  moves value ×1
    Curve → Payout  1 library call
    Curve → Token  2 calls  2 via interface  moves value ×2
    Router → Curve  1 call, 1 read  2 via interface  moves value ×1

  Unresolved
    Hook → IPoolManager  2 calls

  Calls and reads per deployed contract
    Curve  5 → · 2 ←
    DenA  0 → · 1 ←
    DenB  0 → · 1 ←
    Hook  0 → · 2 ←
    Router  2 → · 0 ←
    Token  0 → · 2 ←
```

## route

`route <from> <to> [--reads]`

Prints the shortest chains of calls from one contract to another through
deployed contracts, step by step, with every call of each step. Library and
`this` calls are not followed; reads are followed only with `--reads`. A call
marked `not recorded from the previous step` joins the route by contract only:
nothing records the previous step's callee reaching it inside the contract;
`moves value` and `in try` mark the calls as in `links`. When more than 32
routes are equally short, the rest are left out and the header says so.

```
ilold[Curve]> route Router Hook

  Router → Hook  1 shortest route(s) following calls

  route 1 Router → Curve → Hook
    Router → Curve
      Router::buyWith → Curve::buy  line 14  moves value
    Curve → Hook
      CurveBase::_graduate → Hook::migrate  line 27  moves value
```

## value

`value [contract]`

Who pays whom: the value moves of every deployed project contract, grouped per
running contract by the two sides, the asset and the token. A side is a contract only
where the code resolves one — the contract running the code, the contract a
call with value or a mint or burn goes to, or a contract outside the project
the call is typed as, such as a pool manager (`(outside the project)`); any
other account stays as the source writes it, in backticks. A Uniswap v4
`Currency` is shown as `ETH or token, set by the pool`. With a contract, only
the moves it runs or is a side of are listed.

```
ilold[Curve]> value

  Value flow  6 pair(s), 6 transfer(s)
    as Curve runs
      Curve → Hook  native  ×1  CurveBase::_graduate
      Curve → `payable(to)`  native  ×1  buy
      Curve → `to`  erc20 token  ×1  Payout::pay
      Token → `to`  erc20 token  ×1  buy
    as Hook runs
      Hook → IPoolManager (outside the project)  native  ×1  migrate
    as Router runs
      Router → Curve  native  ×1  buyWith
```

`links`, `route` and `value` read the same facts as the web API
([`/api/contract/{name}/connections`](../reference/api-endpoints.md#get-apicontractnameconnections),
[`/api/project/route`](../reference/api-endpoints.md#get-apiprojectroute),
[`/api/project/value-flow`](../reference/api-endpoints.md#get-apiprojectvalue-flow))
as the MCP tools `ilold_connections`, `ilold_route` and `ilold_value_flow`.

## use

`use <contract>`

Switches the active contract. Every contract keeps its own session (scenarios,
forks and steps), so switching never discards work: `use` back to a contract to
continue where you left it.

```
ilold[Staking]> use Ownable

  ✓ Now using: Ownable
  Staking keeps its 2 step(s); `use Staking` returns to them
```

After switching, all session and analysis commands operate on the new contract.
To start the REPL directly on a contract, pass `ilold explore <path> --contract <name>`.
