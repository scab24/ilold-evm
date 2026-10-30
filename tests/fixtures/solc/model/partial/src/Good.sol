// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract Good {
    uint256 public value;

    function set(uint256 v) external {
        value = v;
    }
}
