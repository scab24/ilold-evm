// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// A compile error inside the sources: no facts, only the file inventory.
contract Bad {
    function f() external pure returns (uint256) {
        uint256 x = "not a number";
        return x;
    }
}
