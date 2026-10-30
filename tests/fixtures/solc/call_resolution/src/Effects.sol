// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface ISink {
    function settle(uint256 amount) external;
}

// Overloaded internal callee whose overloads have different effects (AToken._transfer shape).
contract Ledger {
    uint256 public plain;
    uint256 public settled;
    ISink public sink;
    uint256[] internal stored;

    function _move(uint256 amount) internal {
        plain = amount;
    }

    function _move(uint256 amount, bool notify) internal {
        settled = amount;
        if (notify) sink.settle(amount);
    }

    function moveOne(uint256 amount) external {
        _move(amount);
    }

    function moveTwo(uint256 amount) external {
        _move(amount, true);
    }

    // internal overloads that differ only by data location
    function _sum(uint256[] calldata xs) internal pure returns (uint256 total) {
        for (uint256 i = 0; i < xs.length; i++) total += xs[i];
    }

    function _sum(uint256[] storage xs) internal view returns (uint256) {
        return xs.length;
    }

    function sums(uint256[] calldata xs) external view returns (uint256) {
        return _sum(xs) + _sum(stored);
    }
}

// super inside a diamond continues in the most derived linearization: [SD, SC, SB, SA].
contract SA {
    uint256 public sa;

    function f() public virtual {
        sa = 1;
    }
}

contract SB is SA {
    uint256 public sb;

    function f() public virtual override {
        sb = 1;
        super.f();
    }
}

contract SC is SA {
    uint256 public sc;

    function f() public virtual override {
        sc = 1;
        super.f();
    }
}

contract SD is SB, SC {
    function f() public override(SB, SC) {
        super.f();
    }
}

// Base constructor with the same signature as the child's.
contract CtorBase {
    uint256 public seeded;

    constructor() {
        _seed();
    }

    function _seed() internal {
        seeded = 1;
    }
}

contract CtorChild is CtorBase {
    constructor() {}
}

// Function-pointer fields and function-type parameters; a regular function named fallback.
contract Pointers {
    struct Ops {
        function(uint256) internal pure returns (uint256) op;
    }

    function twice(uint256 x) internal pure returns (uint256) {
        return x * 2;
    }

    function applyOp(uint256 x) external pure returns (uint256) {
        Ops memory o = Ops(twice);
        return o.op(x);
    }

    function withCallback(function(uint256) external returns (bool) cb) external pure returns (bytes4) {
        return cb.selector;
    }

    function fallback() external pure returns (uint256) {
        return 1;
    }
}
