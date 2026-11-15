// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IToken} from "./Interfaces.sol";

library Payout {
    function pay(IToken token, address to, uint256 amount) internal {
        token.transfer(to, amount);
    }
}
