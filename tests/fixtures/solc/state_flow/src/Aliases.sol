// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Types} from "./Types.sol";
import {ExtCounter} from "@extlib/ExtCounter.sol";

struct Node {
    uint256 x;
    mapping(uint256 => Node) kids;
}

interface IOracle {
    function price(address asset) external view returns (uint256);
    function poke(address asset) external;
}

contract Aliases {
    uint256 public constant CAP = 100;
    uint256 public immutable START;
    IOracle public immutable ORACLE;

    mapping(address => Types.Reserve) internal _a;
    mapping(address => Types.Reserve) internal _b;
    mapping(address => Types.Reserve) internal _c;
    mapping(uint256 => uint256) internal slots;
    uint256[] internal list;
    uint256 internal nonce;
    uint256 internal counter;
    uint256 internal seq;
    uint256 internal last;
    uint256 internal twice;

    constructor(IOracle oracle) {
        START = block.timestamp;
        ORACLE = oracle;
    }

    modifier doubled() {
        _;
        _;
    }

    modifier touchAfter(address k) {
        Types.Reserve storage s = _a[k];
        _;
        s.rate = 5;
    }

    // every branch returns into the modifier, which still writes through its pointer
    function bothReturn(address k, bool c) external touchAfter(k) returns (uint256) {
        if (c) {
            return 1;
        } else {
            return 2;
        }
    }

    function bothTry(address k) external touchAfter(k) returns (uint256) {
        try ORACLE.price(k) returns (uint256 p) {
            return p;
        } catch {
            return 0;
        }
    }

    // re-pointed on one branch: both places may be written
    function branchAlias(address k, bool flag) external {
        Types.Reserve storage r = _a[k];
        if (flag) {
            r = _b[k];
        }
        r.rate = 1;
    }

    // re-pointed on both branches: the initial place is no longer reachable
    function elseAlias(address k, bool flag) external {
        Types.Reserve storage r = _a[k];
        if (flag) {
            r = _b[k];
        } else {
            r = _c[k];
        }
        r.index = 2;
    }

    // a branch that reverts does not reach the write
    function revertingBranch(address k, bool flag) external {
        Types.Reserve storage r = _a[k];
        if (flag) {
            r = _b[k];
            revert();
        }
        r.rate = 3;
    }

    // re-pointed inside a loop: later iterations write the new place
    function loopAlias(address k, uint256 n) external {
        Types.Reserve storage r = _a[k];
        for (uint256 i = 0; i < n; i++) {
            r.updatedAt = 1;
            r = _b[k];
        }
        r.index = 4;
    }

    function _store(uint256 id, uint256 v) internal {
        slots[id] = v;
    }

    // increments and assignments nested in call arguments and index expressions
    function nested(uint256 v) external {
        _store(nonce++, v);
        slots[counter++] = v;
        uint256 x;
        x = seq++;
        _store(last = v, x);
    }

    // constants and immutables take no storage
    function useConsts() external view returns (uint256) {
        return CAP + START;
    }

    // only view and static external calls
    function viewExternal(address k) external view returns (uint256) {
        return ORACLE.price(k);
    }

    function staticRaw(address t) external view returns (bool ok) {
        (ok, ) = t.staticcall("");
    }

    function pokeExternal(address k) external {
        ORACLE.poke(k);
    }

    // delete and push/pop through storage pointers
    function pointerOps(address k) external {
        Types.Reserve storage r = _a[k];
        delete r.rate;
        uint256[] storage l = list;
        l.push(1);
        l.pop();
    }

    // an internal (non-library) function writing through a storage parameter
    function _bumpRef(Types.Reserve storage r) internal {
        r.index += 1;
    }

    function viaInternalParam(address k) external {
        _bumpRef(_c[k]);
    }

    // a modifier running the body twice
    function twiceRun() external doubled {
        twice += 1;
    }

    // `this.f()` is an external call; its effects are not followed
    function selfCall(address k) external {
        this.pokeSelf(k);
    }

    function pokeSelf(address k) external {
        _a[k].rate = 9;
    }

    // a type expression in a condition
    function capped(uint256 v) external pure returns (uint256) {
        require(v < type(uint256).max, "max");
        return v;
    }
}

// Mutual recursion: each function reaches everything the other one reaches.
contract Cycle {
    uint256 internal pings;
    uint256 internal pongs;
    uint256 internal side;
    uint256 internal tails;

    function ping(uint256 n) public {
        pings += 1;
        _pingSide();
        if (n > 0) pong(n - 1);
    }

    function _pingSide() internal {
        side += 1;
    }

    function pong(uint256 n) public {
        pongs += 1;
        _tail();
        if (n > 0) ping(n - 1);
    }

    function _tail() internal {
        tails += 1;
    }
}

// A call chain deeper than any fixed depth limit.
contract Deep {
    uint256 internal deep;

    function d0() external { _d1(); }
    function _d1() internal { _d2(); }
    function _d2() internal { _d3(); }
    function _d3() internal { _d4(); }
    function _d4() internal { _d5(); }
    function _d5() internal { _d6(); }
    function _d6() internal { _d7(); }
    function _d7() internal { _d8(); }
    function _d8() internal { _d9(); }
    function _d9() internal { _d10(); }
    function _d10() internal { _d11(); }
    function _d11() internal { _d12(); }
    function _d12() internal { _d13(); }
    function _d13() internal { _d14(); }
    function _d14() internal { _d15(); }
    function _d15() internal { _d16(); }
    function _d16() internal { _d17(); }
    function _d17() internal { _d18(); }
    function _d18() internal { _d19(); }
    function _d19() internal { _d20(); }
    function _d20() internal { _d21(); }
    function _d21() internal { _d22(); }
    function _d22() internal { _d23(); }
    function _d23() internal { _d24(); }
    function _d24() internal { _d25(); }
    function _d25() internal { _d26(); }
    function _d26() internal { _d27(); }
    function _d27() internal { _d28(); }
    function _d28() internal { _d29(); }
    function _d29() internal { _d30(); }
    function _d30() internal { deep = 1; }
}

// A virtual modifier overridden by the derived contract.
contract MarkBase {
    uint256 public baseMarks;
    uint256 public derivedMarks;

    modifier mark() virtual {
        baseMarks += 1;
        _;
    }

    function act() external mark {}
}

contract MarkDerived is MarkBase {
    ExtCounter.Counter internal marks;

    modifier mark() override {
        derivedMarks += 1;
        ExtCounter.decrement(marks);
        _;
    }
}

// each recursive call writes one level deeper, so the written places never stop growing
contract Tree {
    Node internal root;

    function grow(uint256 d) external {
        _walk(root, d);
    }

    function _walk(Node storage n, uint256 d) internal {
        n.x = d;
        if (d > 0) _walk(n.kids[d], d - 1);
    }
}
