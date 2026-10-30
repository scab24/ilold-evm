// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// C3 linearization of D is [D, C, B, A]: super in C continues to B, not A.
contract A {
    function who() public pure virtual returns (string memory) {
        return "A";
    }
}

contract B is A {
    function who() public pure virtual override returns (string memory) {
        return super.who();
    }
}

contract C is A {
    function who() public pure virtual override returns (string memory) {
        return super.who();
    }
}

contract D is B, C {
    function who() public pure override(B, C) returns (string memory) {
        return super.who();
    }
}
