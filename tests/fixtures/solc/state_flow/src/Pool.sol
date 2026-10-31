// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Types} from "./Types.sol";
import {ReserveLib} from "./ReserveLib.sol";
import {SupplyLib} from "./SupplyLib.sol";
import {ExtCounter} from "@extlib/ExtCounter.sol";

contract Pool {
    using ReserveLib for Types.Reserve;
    using ExtCounter for ExtCounter.Counter;

    struct Snapshot {
        uint256 total;
    }

    mapping(address => Types.Reserve) internal _reserves;
    mapping(address => Types.Reserve) internal _otherReserves;
    mapping(address => Types.UserConfig) internal _usersConfig;
    uint256 public total;
    uint256 public totalSupplyCap;
    address public admin;
    ExtCounter.Counter internal hits;

    error NotAdmin();

    modifier onlyAdmin() {
        _onlyAdmin();
        _;
    }

    function _onlyAdmin() internal view {
        if (msg.sender != admin) revert NotAdmin();
    }

    // external library function (delegatecall) writing through a mapping and a struct parameter
    function supply(address asset, uint256 amount) external {
        SupplyLib.executeSupply(_reserves, _usersConfig[msg.sender], asset, amount);
    }

    // internal library function through `using for`: the only effect is a library write
    function touch(address asset, uint256 rate) external {
        _reserves[asset].touch(rate);
    }

    // internal library function named directly
    function bumpDirect(address asset) external {
        ReserveLib.bump(_reserves[asset]);
    }

    // a storage alias reassigned to another root
    function realias(address a, address b) external {
        Types.Reserve storage r = _reserves[a];
        r.rate = 1;
        r = _otherReserves[b];
        r.index = 2;
    }

    function _reserve(address asset) internal view returns (Types.Reserve storage) {
        return _otherReserves[asset];
    }

    // storage pointers returned by an internal function and by a library function
    function viaGetter(address asset) external {
        Types.Reserve storage r = _reserve(asset);
        r.index = 7;
        Types.Reserve storage p = ReserveLib.pick(_reserves, asset);
        p.updatedAt = 1;
    }

    // a modifier whose helper reverts
    function setAdmin(address a) external onlyAdmin {
        admin = a;
    }

    // a local shadowing a state variable is neither read nor written as state
    function shadow() external pure returns (uint256) {
        uint256 total = 5;
        total = total + 1;
        return total;
    }

    // a state variable whose name contains another one; a struct field named like one
    function capOnly() external view returns (uint256) {
        Snapshot memory s;
        s.total = 1;
        return totalSupplyCap + s.total;
    }

    // dependency library from lib/, internal and public
    function hit() external {
        hits.increment();
    }

    function clear() external {
        ExtCounter.reset(hits);
    }
}
