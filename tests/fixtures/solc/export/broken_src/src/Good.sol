// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

contract Good {
    uint256 public value;

    function set(uint256 next) external {
        value = next;
    }
}
