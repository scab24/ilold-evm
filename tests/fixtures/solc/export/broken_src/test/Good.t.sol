// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Good} from "../src/Good.sol";

contract GoodTest {
    function testSet() external {
        new Good().set(1);
    }
}
