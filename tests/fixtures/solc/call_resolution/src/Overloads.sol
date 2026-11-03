// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

contract OverBase {
    function put(uint256 a, bool flag) public virtual returns (uint256) {
        return flag ? a : 0;
    }
}

// Two own overloads plus one inherited, and the special functions.
contract Over is OverBase {
    uint256 public last;

    constructor() {
        last = 1;
    }

    function put(uint256 a) public returns (uint256) {
        last = a;
        return put(a, true);
    }

    function put(address who) public returns (uint256) {
        return put(uint160(who));
    }

    receive() external payable {}

    fallback() external payable {}
}
