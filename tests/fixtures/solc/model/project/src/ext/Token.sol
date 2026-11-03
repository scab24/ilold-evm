// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IToken as CoreToken} from "../core/IToken.sol";

// Same name as src/core/Token.sol; inherits the core interface through an alias.
contract Token is CoreToken {
    uint256 public held;

    function balance() external view returns (uint256) {
        return held;
    }

    function mint(uint256 amount) external {
        held += amount;
    }

    function burn(uint256 amount) external {
        held -= amount;
    }
}
