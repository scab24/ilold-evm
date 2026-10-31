// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// the internal overload is declared before the callable one
contract EntryOverload {
    uint256 public total;

    function add(uint256 a, uint256 b) internal pure returns (uint256) {
        return a + b;
    }

    function add(uint256 a) external {
        total = add(total, a);
    }
}
