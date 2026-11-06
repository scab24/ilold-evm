// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IVault, IHook} from "./Interfaces.sol";

contract Vault is IVault {
    uint256 public total;

    function deposit(uint256 amount) external {
        total += amount;
    }
}

contract HookA is IHook {
    function onCall(uint256 x) external pure returns (uint256) {
        return x + 1;
    }
}

contract HookB is IHook {
    function onCall(uint256 x) external pure returns (uint256) {
        return x * 2;
    }
}

contract Registry {
    function lookup(bytes32 key) external pure returns (address) {
        return address(uint160(uint256(key)));
    }
}
