// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Types} from "./Types.sol";

// Internal library functions writing through `storage` struct parameters.
library ReserveLib {
    error Stale();

    function touch(Types.Reserve storage reserve, uint256 rate) internal {
        reserve.rate = rate;
        reserve.updatedAt = uint40(block.timestamp);
    }

    function bump(Types.Reserve storage reserve) internal {
        if (reserve.updatedAt == 0) revert Stale();
        _accrue(reserve);
    }

    function _accrue(Types.Reserve storage r) private {
        r.index += 1;
    }

    // a storage pointer derived from a parameter
    function pick(mapping(address => Types.Reserve) storage all, address asset)
        internal
        view
        returns (Types.Reserve storage)
    {
        return all[asset];
    }
}
