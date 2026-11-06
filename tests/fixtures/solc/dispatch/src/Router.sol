// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IOracle} from "@ext/IOracle.sol";
import {IVault, IHook, IOrphan, IRegistry} from "./Interfaces.sol";

contract Router {
    IVault public immutable vault;
    IHook public hook;
    IOracle public oracle;
    IOrphan public orphan;
    IRegistry public registry = IRegistry(address(0x1234));

    constructor(IVault v, IHook h, IOracle o) {
        vault = v;
        hook = h;
        oracle = o;
        orphan = IOrphan(address(0));
    }

    function initialize(IOrphan o) external {
        orphan = o;
    }

    function setHook(IHook h) external {
        hook = h;
    }

    function run(uint256 x) external returns (uint256) {
        vault.deposit(x);
        uint256 y = hook.onCall(x);
        orphan.ping();
        registry.lookup(bytes32(x));
        return y + oracle.price() + this.selfValue();
    }

    function selfValue() external pure returns (uint256) {
        return 1;
    }

    function viaPointer(uint256 x) external {
        function(uint256) external f = vault.deposit;
        f(x);
    }
}
