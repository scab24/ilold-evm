// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Context} from "./Context.sol";

abstract contract Multicall is Context {
    function multicall(bytes[] calldata data) external virtual returns (bytes[] memory results) {
        results = new bytes[](data.length);
        for (uint256 i = 0; i < data.length; i++) {
            (bool ok, bytes memory out) = address(this).delegatecall(data[i]);
            require(ok);
            results[i] = out;
        }
    }
}
