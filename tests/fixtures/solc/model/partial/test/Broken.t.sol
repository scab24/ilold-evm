// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Good} from "../src/Good.sol";

// A broken test must not discard the model of src.
contract BrokenTest {
    function testSet() external {
        uint256 x = "not a number";
        new Good().set(x);
    }
}
