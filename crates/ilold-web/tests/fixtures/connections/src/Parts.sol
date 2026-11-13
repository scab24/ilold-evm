// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IPoolManager, PoolMath} from "@pool/IPoolManager.sol";
import {IDen, IHook, IToken} from "./Interfaces.sol";

contract Token is IToken {
    function transfer(address, uint256) external pure returns (bool) {
        return true;
    }

    function allowance(address, address) external pure returns (uint256) {
        return 0;
    }

    function mint(address, uint256) external {}
}

contract DenA is IDen {
    function deliver(address) external {}
}

contract DenB is IDen {
    function deliver(address) external {}
}

contract Hook is IHook {
    IPoolManager public manager;

    constructor(IPoolManager manager_) {
        manager = manager_;
    }

    function migrate(uint256 amount) external payable {
        manager.unlock(abi.encode(amount));
        manager.settle{value: amount}();
    }

    function fee() external pure returns (uint256) {
        return PoolMath.half(2);
    }
}
