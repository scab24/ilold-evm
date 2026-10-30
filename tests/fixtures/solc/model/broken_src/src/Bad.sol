// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract Bad {
    function f() external pure returns (uint256) {
        uint256 x = "not a number";
        return x;
    }
}
