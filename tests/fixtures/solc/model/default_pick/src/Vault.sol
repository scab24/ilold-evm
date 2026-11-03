// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

abstract contract Base {
    uint256 public total;
}

contract Vault is Base {
    function deposit(uint256 amount) external {
        total += amount;
    }
}
