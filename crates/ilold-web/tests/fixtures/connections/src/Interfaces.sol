// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface ICurve {
    function buy(address to) external payable;
    function migrate() external;
    function price() external view returns (uint256);
}

interface IHook {
    function migrate(uint256 amount) external payable;
    function fee() external view returns (uint256);
}

interface IToken {
    function transfer(address to, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
    function mint(address to, uint256 amount) external;
}

interface IDen {
    function deliver(address to) external;
}
