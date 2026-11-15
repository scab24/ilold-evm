// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IX {
    function f() external;
}

abstract contract Impl is IX {
    function f() external {}
}

contract A is Impl {}

contract B is Impl {}

contract Caller {
    function go(address x) external {
        IX(x).f();
    }
}
