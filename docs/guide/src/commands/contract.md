# Contract Commands

Contract commands inspect the structure of the loaded contracts without modifying the session.

## functions

`f` or `functions`

Lists the callable functions in the active contract with their access level and tags.

```
ilold[Staking]> f

  [public] deposit           writes state
  [public] withdraw          writes state, external calls
  [public] claimRewards      writes state, external calls
  [public] getStakeInfo      view
  [restricted(onlyOwner)] pause    writes state
  [restricted(onlyOwner)] unpause  writes state
```

Tags indicate `writes state`, `external calls`, or `view` (read-only, no external calls).

## funcs-all

`fa` or `funcs-all`

Lists all accessible functions including those inherited from parent contracts.

```
ilold[Staking]> fa

  [public] deposit           writes state
  [public] withdraw          writes state, external calls
  [public] claimRewards      writes state, external calls
  [public] getStakeInfo      view
  [restricted(onlyOwner)] pause    writes state
  [restricted(onlyOwner)] unpause  writes state

  inherited:
  [public] owner              from Ownable
  [public] transferOwnership  from Ownable
```

Inherited functions are listed separately with their origin contract.

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

`ct` or `contracts`

Lists all contracts in the loaded project with their type, function count, state variable count, and inheritance.

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
