// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// Inherited dependency code: reachable from the project.
abstract contract Ownable {
    address public owner;

    event OwnerSet(address indexed owner);

    modifier onlyOwner() {
        require(msg.sender == owner, "owner");
        _;
    }

    function _setOwner(address next) internal {
        owner = next;
        emit OwnerSet(next);
    }
}
