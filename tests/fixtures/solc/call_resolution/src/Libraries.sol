// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

library MathLib {
    function double(uint256 x) internal pure returns (uint256) {
        return x * 2;
    }

    function triple(uint256 x) external pure returns (uint256) {
        return x * 3;
    }
}

contract UsesLib {
    using MathLib for uint256;

    uint256 public stored;

    function viaUsing(uint256 x) external {
        stored = x.double();
    }

    function direct(uint256 x) external {
        stored = MathLib.double(x);
    }

    function delegated(uint256 x) external {
        stored = MathLib.triple(x);
    }
}
