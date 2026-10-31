// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// both `put` functions are keyed `put`: keys are unique only within the declaring contract
contract ClashBase {
    address public owner;
    uint256 public bumps;

    modifier onlyOwner() {
        require(msg.sender == owner, "owner");
        _;
    }

    function put(uint256 v) public onlyOwner {
        _bump(v);
    }

    function _bump(uint256 v) internal {
        bumps += v;
    }
}

contract Clash is ClashBase {
    address public who;

    function put(address a) public {
        who = a;
    }
}
