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

A check counts only when its condition must hold for the caller to get through: `require(msg.sender != owner)` and `if (msg.sender == owner) revert` let everyone but the owner in and are not access guards, and `require(msg.sender == admin || open)` is not one either because anyone passes while `open` is true. A call that takes the caller is a role check only when the called function is `view` or `pure`; `require(token.transferFrom(msg.sender, ...))` is not.

Guards are read along the straight-line entry path only. Checks inside loops, inside `if`/`else` branches that do not simply revert, or after a branch, loop or assembly block that may return early are not reported, so a function can be `[P]` and still check its caller on some paths. Check its trace (`tr`) or source before concluding it is unguarded.

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

Tags are `mutable`, `const`, or `immutable`.

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

```
ilold[Vault]> deps

  Vault → depends on
    IPool     calls×2 holds
    SafeMath  calls×2
```

Here `Vault` calls into `IPool` twice and keeps an `IPool` state variable
(`holds`), and calls the `SafeMath` library through a `using` directive.

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
