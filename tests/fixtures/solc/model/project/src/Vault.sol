// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable} from "@ozlike/access/Ownable.sol";
import {Multicall} from "@ozlike/utils/Multicall.sol";
import {Math} from "@ozlike/utils/Math.sol";
import {VaultBase} from "./VaultBase.sol";
import {IVault} from "./IVault.sol";

contract Vault is VaultBase, Ownable, Multicall, IVault {
    // implements IVault.ADDRESSES through its getter
    address public immutable override ADDRESSES;
    uint256 public total;

    constructor(address provider) Ownable(msg.sender) {
        ADDRESSES = provider;
    }

    function deposit(uint256 amount) external onlyOwner {
        total += Math.max(_helper(amount), baseVar);
    }
}
