// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Scoped} from "@scoped/token/Scoped.sol";
import {Dep} from "dep/Dep.sol";

contract App {
    function total(Scoped scoped) external view returns (uint256) {
        return scoped.value() + Dep.outerValue();
    }
}
