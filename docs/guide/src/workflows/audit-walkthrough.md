# Full Audit Walkthrough

This walkthrough demonstrates a realistic audit of a Staking contract using the ilold interactive REPL. The contract allows users to deposit tokens, withdraw them, and claim rewards, with owner-only administrative functions and a pause mechanism.

## Starting the session

Launch ilold against the Staking Foundry project:

```
ilold explore tests/fixtures/staking
```

The REPL starts and auto-selects the main contract:

```
ilold explore -- Staking
8 functions | Type ? for help
Web UI: http://127.0.0.1:3001

Staking >
```

List available functions and state variables to orient yourself:

```
Staking > f
  deposit          External   writes state   
  withdraw         External   writes state   
  claimRewards     External   writes state   
  setRewardRate    External   writes state   onlyOwner
  pause            External   writes state   onlyOwner
  unpause          External   writes state   onlyOwner
  rewardPerToken   Public     read-only
  earned           Public     read-only

Staking > v
  stakingToken       address
  rewardToken        address
  owner              address
  paused             bool
  rewardRate         uint256
  lastUpdateTime     uint256
  rewardPerTokenStored  uint256
  balances           mapping(address => uint256)
  userRewardPerTokenPaid  mapping(address => uint256)
  rewards            mapping(address => uint256)
  totalStaked        uint256
```

The function list shows access level, whether a function writes state, and which modifiers restrict access. Read-only functions (view/pure) are marked separately. This is your starting map.

## Calling deposit -- observing state writes

```
Staking > c deposit
  Step 0: deposit [External]
  State changed: balances, totalStaked
```

The session records `deposit` as step 0. The engine analyzed the CFG and reports which state variables are mutated. Two writes: `balances` and `totalStaked`.

## Investigating totalStaked with who, slice, trace

The `who` command reveals which functions read and write a variable across the entire contract:

```
Staking > who totalStaked
  Writers:
    deposit          External
    withdraw         External
  Readers:
    rewardPerToken   Public
```

Both `deposit` and `withdraw` modify `totalStaked`, and `rewardPerToken` reads it. This tells you the impact surface: any bug in how `totalStaked` is updated will propagate to reward calculations.

Now use `slice` to see the dataflow within `deposit`:

```
Staking > sl deposit totalStaked
  Backward slice (sources -> totalStaked):
    [0] stakingToken.transferFrom(msg.sender, address(this), amount)
    [2] totalStaked += amount

  Forward slice (totalStaked -> sinks):
    [2] totalStaked += amount
```

The backward slice shows that the `transferFrom` call precedes the state write. The forward slice is short because `totalStaked` is only written, not read within this function. The real consumers are in `rewardPerToken` -- the `who` output already told you that.

Use `trace` to see the full execution flow:

```
Staking > tr deposit
  1. [mod whenNotPaused] require(!paused, "paused")
  2. [mod updateReward] rewardPerTokenStored = rewardPerToken()
  3. [mod updateReward] lastUpdateTime = block.timestamp
  4. [mod updateReward] userRewardPerTokenPaid[account] = rewardPerTokenStored
  5. [mod updateReward] rewards[account] = earned(account)
  6. require(amount > 0, "Cannot stake 0")
  7. stakingToken.transferFrom(msg.sender, address(this), amount)
  8. balances[msg.sender] += amount
  9. totalStaked += amount
```

Lines prefixed with `[mod ...]` come from modifiers inlined before the function body. The `updateReward` modifier writes four state variables before `deposit`'s own body runs. This is critical context: if you only looked at the function body, you would miss these writes.

## Calling withdraw -- observing the sequence

```
Staking > c withdraw
  Step 1: withdraw [External]
  State changed: balances, totalStaked

Staking[deposit > withdraw] >
```

The prompt now shows both steps. Use `state` to see the accumulated picture:

```
Staking[deposit > withdraw] > s
  balances             written by: deposit (+= amount), withdraw (-= amount)
  totalStaked          written by: deposit (+= amount), withdraw (-= amount)
  rewardPerTokenStored written by: deposit (via updateReward), withdraw (via updateReward)
  lastUpdateTime       written by: deposit (via updateReward), withdraw (via updateReward)
  userRewardPerTokenPaid written by: deposit (via updateReward), withdraw (via updateReward)
  rewards              written by: deposit (via updateReward), withdraw (via updateReward)
```

The state view aggregates every write from every step. Mutations introduced by modifiers are tagged with `via <modifier>`. You can see that `updateReward` runs in both `deposit` and `withdraw`, updating the reward accounting state each time.

## Checking claimRewards -- modifier writes in the slice

```
Staking > c claimRewards
  Step 2: claimRewards [External]
  State changed: rewards
```

Slice `claimRewards` for the `rewards` variable:

```
Staking[deposit > withdraw > claimRewards] > sl claimRewards rewards --both
  Backward slice (sources -> rewards):
    [mod updateReward] rewards[account] = earned(account)
    [5] uint256 reward = rewards[msg.sender]

  Forward slice (rewards -> sinks):
    [mod updateReward] rewards[account] = earned(account)
    [5] uint256 reward = rewards[msg.sender]
    [7] rewards[msg.sender] = 0
    [8] rewardToken.transfer(msg.sender, reward)
```

Entries tagged `[mod updateReward]` are statements from the modifier body that touch `rewards`. The slicer walks both the function body and every applied modifier, so nothing is hidden. The forward slice shows that `rewards[msg.sender]` is read into a local, zeroed, and then transferred -- the standard claim pattern.

## Using timeline to track a variable across all steps

```
Staking[deposit > withdraw > claimRewards] > tl balances
  Variable: balances

  Step 0  deposit       balances[msg.sender] += amount
  Step 1  withdraw      balances[msg.sender] -= amount
```

```
Staking[deposit > withdraw > claimRewards] > tl rewardPerTokenStored
  Variable: rewardPerTokenStored

  Step 0  deposit       rewardPerTokenStored = rewardPerToken()   via updateReward
  Step 1  withdraw      rewardPerTokenStored = rewardPerToken()   via updateReward
  Step 2  claimRewards  rewardPerTokenStored = rewardPerToken()   via updateReward
```

The timeline shows every mutation of a variable across the entire session in chronological order. Each entry includes the step index, the function that caused it, the assignment expression, and whether it came from a modifier. Path conditions (from branching logic) are included when the function has conditional writes.

This gives you a cross-function view that no single-function analysis can provide.

## Recording findings and exporting

Record an observation while looking at the claim flow:

```
Staking[deposit > withdraw > claimRewards] > n claimRewards zeroes rewards before transfer -- CEI pattern followed
```

If you spot an issue, record a finding with severity:

```
Staking[deposit > withdraw > claimRewards] > fi medium No check that reward > 0 before transfer call
  Finding F-001 added (Medium)
```

Mark functions as reviewed:

```
Staking[deposit > withdraw > claimRewards] > status deposit reviewed
Staking[deposit > withdraw > claimRewards] > status claimRewards suspicious
```

Export the session to a markdown report:

```
Staking[deposit > withdraw > claimRewards] > export
  Exported to ilold-report-Staking.md
```

Save the session for later:

```
Staking[deposit > withdraw > claimRewards] > save staking-audit-day1
  Saved to ~/.ilold/sessions/staking-audit-day1.json
```

## How cross-reference hints guide exploration

The output of each command naturally points to the next:

1. `functions` shows which functions write state -- you call the important ones first.
2. `c deposit` reports `balances, totalStaked` changed -- you run `who totalStaked` to see the impact.
3. `who` shows `rewardPerToken` reads `totalStaked` -- you run `sl rewardPerToken totalStaked` or `tr rewardPerToken` to trace the dependency.
4. `slice` shows modifier-origin statements -- you trace the modifier with `tr deposit` to see full execution order.
5. `state` aggregates writes across steps -- variables with writes from multiple functions deserve `timeline` inspection.
6. `timeline` reveals the chronological mutation history -- unexpected patterns become findings.

Each command's output contains the variable names, function names, and modifier names needed for the next query. The workflow is: call, observe, investigate, record, repeat.

## Related pages

- [Session commands](../commands/session.md)
- [Taint Analysis](./taint-analysis.md)
- [HTTP API Reference](../reference/api-endpoints.md)
- [Known Limitations](../reference/limitations.md)
