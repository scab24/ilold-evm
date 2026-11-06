// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// One implementation in scope.
interface IVault {
    function deposit(uint256 amount) external;
}

/// Two implementations in scope.
interface IHook {
    function onCall(uint256 x) external returns (uint256);
}

/// No implementation in scope.
interface IOrphan {
    function ping() external;
}

/// Implemented only by a contract that does not inherit it.
interface IRegistry {
    function lookup(bytes32 key) external view returns (address);
}
