// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

library Dep {
    function innerValue() internal pure returns (uint256) {
        return 2;
    }
}
