// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// Control flow shapes the CFG builder must mirror exactly.
interface IAcl {
    function isAdmin(address who) external view returns (bool);
}

interface IProvider {
    function acl() external view returns (address);
    function check(uint256 x) external view returns (bool);
}

contract Base {
    uint256 public baseValue;

    constructor(uint256 v) {
        baseValue = v;
    }
}

contract ControlFlow is Base {
    error TooLow(uint256 x);
    event Seen(uint256 x);

    IProvider public provider;
    uint256 public total;

    modifier guarded() {
        require(total < 100, "guard");
        _;
    }

    constructor(IProvider p) Base(1) guarded {
        provider = p;
    }

    function ifRevert(uint256 x) external {
        if (x < 10) {
            revert TooLow(x);
        }
        total = x;
    }

    function revertCall(uint256 x) external {
        if (x == 0) revert("zero");
        total = x;
    }

    function revertLast(uint256 x) external {
        total = x;
        revert TooLow(x);
    }

    function forBreakContinue(uint256 n) external {
        for (uint256 i = 0; i < n; i++) {
            if (i == 2) continue;
            if (i == 5) break;
            total += i;
        }
    }

    function whileContinue(uint256 n) external {
        uint256 i = 0;
        while (i < n) {
            i++;
            if (i == 3) continue;
            total += i;
        }
    }

    function doWhileBreakContinue(uint256 n) external {
        uint256 i = 0;
        do {
            i++;
            if (i == 2) continue;
            if (i == n) break;
            total += i;
        } while (i < 5);
    }

    function nested(uint256 n) external {
        for (uint256 i = 0; i < n; i++) {
            for (uint256 j = 0; j < n; j++) {
                if (j == 1) continue;
                if (j == 2) break;
                total += j;
            }
            if (i == 3) break;
        }
    }

    function localInc(uint256 a) external returns (uint256) {
        uint256 x = a;
        x++;
        x += 2;
        total++;
        return x;
    }

    function condCalls(uint256 x) external {
        if (provider.check(x)) {
            total = 1;
        }
        while (_ok(x)) {
            x--;
        }
        for (uint256 i = 0; _ok(i); i++) {
            total = i;
        }
        do {
            x--;
        } while (_ok(x));
    }

    function requireCalls(uint256 x) external view {
        require(IAcl(provider.acl()).isAdmin(msg.sender), "admin");
        require(_ok(x), TooLow(_val(x)));
    }

    function emitRevertCalls(uint256 x) external {
        emit Seen(_val(x));
        if (x == 7) revert TooLow(_val2(x));
        total = x > 1 ? _val(x) : _val2(x);
    }

    function _ok(uint256 x) internal pure returns (bool) {
        return x > 0;
    }

    function _val(uint256 x) internal pure returns (uint256) {
        return x;
    }

    function _val2(uint256 x) internal pure returns (uint256) {
        return x + 1;
    }
}
