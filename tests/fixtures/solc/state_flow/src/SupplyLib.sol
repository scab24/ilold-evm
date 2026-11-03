// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Types} from "./Types.sol";
import {ReserveLib} from "./ReserveLib.sol";

// Reached with DELEGATECALL, so its writes land in the caller's storage.
library SupplyLib {
    using ReserveLib for Types.Reserve;

    event Supplied(address asset, uint256 amount);

    function executeSupply(
        mapping(address => Types.Reserve) storage reservesData,
        Types.UserConfig storage userConfig,
        address asset,
        uint256 amount
    ) external {
        Types.Reserve storage reserve = reservesData[asset];
        require(amount != 0, "zero");
        reserve.touch(amount);
        userConfig.data |= 1;
        emit Supplied(asset, amount);
    }
}
