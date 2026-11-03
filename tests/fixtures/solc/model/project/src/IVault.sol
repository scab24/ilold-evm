// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IVault {
    function ADDRESSES() external view returns (address);
    function deposit(uint256 amount) external;
}
