// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ManualLib} from "manual/ManualLib.sol";

contract UsesManual {
    function value(uint256 input) external pure returns (uint256) {
        return ManualLib.double(input);
    }
}
