// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Types} from "./Types.sol";

// Free function bound with `using {f} for T`.
function addTo(Types.Reserve storage r, uint256 v) {
    r.index += v;
}

contract FreeUser {
    using {addTo} for Types.Reserve;

    mapping(address => Types.Reserve) internal pools;

    function add(address a, uint256 v) external {
        pools[a].addTo(v);
    }
}
