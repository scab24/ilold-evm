// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IToken} from "./IToken.sol";

contract Token is IToken {
    uint256 public supply;

    function balance() external view returns (uint256) {
        return supply;
    }

    function mint(uint256 amount) external {
        supply += amount;
    }

    function burn(uint256 amount) external {
        supply -= amount;
    }
}
