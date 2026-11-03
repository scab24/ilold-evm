// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// Resolves only through the `lib/outer/lib/inner/:dep/` context remapping.
import {Dep} from "dep/Dep.sol";

contract Scoped {
    function value() external pure returns (uint256) {
        return Dep.innerValue();
    }
}
