// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// Every Solidity operator and literal kind, for the expression mapping.
contract Ops {
    function arith(uint256 a, uint256 b) external pure returns (uint256 r) {
        r = a + b;
        r = a - b;
        r = a * b;
        r = a / b;
        r = a % b;
        r = a ** b;
        unchecked {
            r += a;
            r -= b;
            r *= a;
            r /= b;
            r %= a;
        }
        r &= a;
        r |= b;
        r ^= a;
        r <<= 1;
        r >>= 1;
        r = a & b;
        r = a | b;
        r = a ^ b;
        r = a << 1;
        r = a >> 1;
        r = ~a;
        r = a > b ? a : b;
        bool c = (a == b) || ((a != b) && (a < b)) || (a > b) || (a <= b) || (a >= b);
        c = !c;
        int256 n = -int256(a);
        r++;
        r--;
        ++r;
        --r;
        delete n;
    }

    function literals() external pure returns (address, string memory, bytes memory, bool, uint256) {
        return (0x5B38Da6a701c568545dCfcB03FcB875f56beddC4, unicode"héllo ✓", hex"00ff", true, 7);
    }
}
