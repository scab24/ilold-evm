// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IToken {
    function balance() external view returns (uint256);
    function mint(uint256 amount) external;
    function burn(uint256 amount) external;
}
