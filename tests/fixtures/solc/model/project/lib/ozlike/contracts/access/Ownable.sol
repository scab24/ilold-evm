// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Context} from "../utils/Context.sol";

abstract contract Ownable is Context {
    address private _owner;

    error NotOwner(address account);
    event OwnershipTransferred(address indexed previous, address indexed next);

    constructor(address initialOwner) {
        _owner = initialOwner;
    }

    modifier onlyOwner() {
        if (_owner != _msgSender()) revert NotOwner(_msgSender());
        _;
    }

    function owner() public view returns (address) {
        return _owner;
    }

    function _transferOwnership(address next) internal {
        emit OwnershipTransferred(_owner, next);
        _owner = next;
    }
}
