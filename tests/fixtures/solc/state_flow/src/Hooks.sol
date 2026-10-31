// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IHook {
    function finalize(uint256 amount) external;
}

// An inherited function calling a virtual hook that the child overrides.
contract TokenBase {
    uint256 public moved;

    function transfer(uint256 amount) external {
        _transfer(amount);
    }

    function _transfer(uint256 amount) internal virtual {
        moved = amount;
    }
}

contract Token is TokenBase {
    IHook public hook;

    function _transfer(uint256 amount) internal override {
        super._transfer(amount);
        hook.finalize(amount);
    }
}

// Code after the placeholder runs after the body on every non-revert exit.
contract Modded {
    uint256 public entered;
    uint256 public exited;

    modifier tracked() {
        entered += 1;
        _;
        exited += 1;
    }

    modifier checked() {
        require(entered < 10, "busy");
        _;
    }

    function early(uint256 x) external tracked returns (uint256) {
        if (x == 0) {
            return 1;
        }
        return 2;
    }

    function both(uint256 x) external checked tracked returns (uint256) {
        return x;
    }
}
