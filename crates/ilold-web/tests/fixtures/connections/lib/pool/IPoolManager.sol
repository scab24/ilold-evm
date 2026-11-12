// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IPoolManager {
    function unlock(bytes calldata data) external returns (bytes memory);
    function settle() external payable returns (uint256);
}
