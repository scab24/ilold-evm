// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// A super chain, a virtual hook called from the base and an inherited modifier that makes a call.
abstract contract Root {
    uint256 internal total;
    uint256 internal tracked;

    modifier track() {
        _track();
        _;
    }

    function _track() internal {
        tracked += 1;
    }

    function _move(uint256 amount) internal virtual {
        total += amount;
    }

    function hook() internal virtual {}

    function run(uint256 amount) public {
        hook();
        _move(amount);
    }

    function value() public view virtual returns (uint256) {
        return total;
    }
}

abstract contract Middle is Root {
    function _move(uint256 amount) internal virtual override {
        super._move(amount + 1);
    }
}

contract Leaf is Middle {
    uint256 public hooks;

    function _move(uint256 amount) internal override {
        super._move(amount * 2);
    }

    function hook() internal override {
        hooks += 1;
    }

    function move(uint256 amount) external track {
        _move(amount);
    }

    function value() public view override returns (uint256) {
        return super.value() + this.extra();
    }

    function extra() external pure returns (uint256) {
        return 1;
    }
}
