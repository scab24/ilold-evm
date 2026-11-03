// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

library Math {
    function max(uint256 a, uint256 b) internal pure returns (uint256) {
        return a > b ? a : b;
    }
}
