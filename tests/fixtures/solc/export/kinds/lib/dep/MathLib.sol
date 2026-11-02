// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// Only `twice` is called from the project.
library MathLib {
    error Overflow(uint256 x);

    function twice(uint256 x) internal pure returns (uint256) {
        if (x > type(uint128).max) revert Overflow(x);
        return x * 2;
    }

    function thrice(uint256 x) internal pure returns (uint256) {
        return x * 3;
    }
}

interface IFeed {
    function price() external view returns (uint256);
}
