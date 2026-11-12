// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {ICurve} from "./Interfaces.sol";

contract Router {
    ICurve public curve;

    constructor(ICurve curve_) {
        curve = curve_;
    }

    function buyWith(address to) external payable {
        curve.buy{value: msg.value}(to);
    }

    function quote() external view returns (uint256) {
        return curve.price();
    }
}
