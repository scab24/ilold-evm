// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

abstract contract VaultBase {
    uint256 internal baseVar;

    function _helper(uint256 x) internal pure returns (uint256) {
        return x + 1;
    }

    function _secret() private pure returns (uint256) {
        return 42;
    }

    function useSecret() internal pure returns (uint256) {
        return _secret();
    }
}
