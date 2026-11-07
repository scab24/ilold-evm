// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

/// A pause flag: tested by a modifier, set by a setter.
abstract contract Pausable {
    bool public paused;
    address public owner;

    modifier whenNotPaused() {
        require(!paused, "paused");
        _;
    }

    function setPaused(bool value) external {
        require(msg.sender == owner, "owner");
        paused = value;
    }
}

/// An enum state machine, a round counter compared with a constant, and variables that are not phases.
contract Sale is Pausable {
    enum State {
        Setup,
        Open,
        Closed
    }

    error RoundsOver();

    uint256 public constant FINAL_ROUND = 3;

    State public state;
    uint256 public round;
    uint256 public raised;
    uint256 public cap;
    // tested by one function only
    bool public audited;
    // tested by two functions, written by none
    bool public frozen;

    constructor(uint256 cap_) {
        owner = msg.sender;
        cap = cap_;
    }

    function open() external {
        require(state == State.Setup, "not setup");
        state = State.Open;
    }

    function buy() external payable whenNotPaused {
        require(state == State.Open, "not open");
        require(raised < cap, "cap");
        _checkRound();
        raised += msg.value;
    }

    function refund() external whenNotPaused {
        if (state != State.Closed) revert("not closed");
        require(raised < cap, "cap");
        raised = 0;
    }

    function close() external {
        require(state == State.Open, "not open");
        state = State.Closed;
    }

    function nextRound() external {
        require(round != FINAL_ROUND, "last");
        round += 1;
    }

    function audit() external {
        audited = true;
    }

    function report() external view returns (uint256) {
        if (audited) return raised;
        return 0;
    }

    function frozenRaised() external view returns (uint256) {
        require(!frozen, "frozen");
        return raised;
    }

    function frozenCap() external view returns (uint256) {
        require(!frozen, "frozen");
        return cap;
    }

    function _checkRound() internal view {
        if (round == FINAL_ROUND) revert RoundsOver();
    }
}

/// Inherits the pause flag: its functions count in this contract too.
contract Vault is Pausable {
    uint256 public total;

    function deposit() external payable whenNotPaused {
        total += msg.value;
    }

    function withdraw(uint256 amount) external whenNotPaused {
        total -= amount;
    }
}
