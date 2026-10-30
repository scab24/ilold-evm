// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

library Dep {
    function outerValue() internal pure returns (uint256) {
        return 1;
    }
}
