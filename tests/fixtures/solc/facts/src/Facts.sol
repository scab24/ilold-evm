// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// Facts a code map needs and must never drop silently.
interface IPool {
    function supply(address asset, uint256 amount) external;
}

// Many paths before the else branch: a small path cap never walks it.
contract PathCap {
    uint256 public a;
    uint256 public b;
    uint256 public c;
    uint256 public rare;

    function branches(uint256 x) external {
        if (x > 0) {
            if (x & 1 == 1) { a = 1; } else { a = 2; }
            if (x & 2 == 2) { b = 1; } else { b = 2; }
            if (x & 4 == 4) { c = 1; } else { c = 2; }
        } else {
            rare = 1;
        }
    }
}

// solc holds Yul to the function's mutability: pure reads and writes nothing, view writes nothing.
contract Asm {
    uint256 public stored;

    modifier viaAsm() {
        assembly { let x := 1 }
        _;
    }

    function pureAsm(uint256 a) public pure returns (uint256 r) {
        assembly { r := add(a, 1) }
    }

    function raw() public view returns (uint256 r) {
        assembly { r := sload(0) }
    }

    function writesAsm() public {
        assembly { sstore(0, 1) }
    }

    function callsPure(uint256 a) external returns (uint256) {
        stored = a;
        return pureAsm(a);
    }

    function callsRaw() external view returns (uint256) {
        return raw();
    }

    function callsWrites() external {
        writesAsm();
    }

    function guarded() external viaAsm {
        stored = 1;
    }

    function plain() external {
        stored = 2;
    }
}

contract Guards {
    bytes32 public constant ADMIN = keccak256("ADMIN");
    address public owner;
    uint256 public value;
    mapping(bytes32 => mapping(address => bool)) internal roles;

    constructor() {
        owner = msg.sender;
    }

    function hasRole(bytes32 role, address account) public view returns (bool) {
        return roles[role][account];
    }

    function _owner() internal view returns (address) {
        return owner;
    }

    modifier onlyRole(bytes32 role) {
        require(hasRole(role, msg.sender), "role");
        _;
    }

    // its name suggests access control, but its code checks the caller on one branch only
    modifier onlyOwnerAsm(uint256 v) {
        if (v > 1) {
            require(msg.sender == owner, "owner");
        }
        assembly { let x := 1 }
        _;
    }

    function inLoop(uint256 n) external {
        for (uint256 i; i < n; i++) {
            require(msg.sender == owner, "loop");
        }
        value = n;
    }

    function inTry(IPool p) external {
        try p.supply(address(0), 1) {
            require(msg.sender == owner, "try");
        } catch {}
        value = 5;
    }

    function roleViaModifier() external onlyRole(ADMIN) {
        value = 6;
    }

    function asmModifierBranch(uint256 v) external onlyOwnerAsm(v) {
        value = v;
    }

    function writeThenCheck(uint256 v) external {
        value = v;
        require(msg.sender == owner, "owner");
    }

    function oneBranch(uint256 v) external {
        if (v > 10) {
            require(msg.sender == owner, "owner");
        }
        value = v;
    }

    function toCheck(address to) external {
        require(msg.sender == to, "to");
        value = 1;
    }

    function viaGetter() external {
        require(_owner() == msg.sender, "owner");
        value = 2;
    }

    function roleCheck() external {
        require(hasRole(ADMIN, msg.sender), "role");
        value = 3;
    }

    function isA(address who) public view returns (bool) {
        return roles[keccak256("A")][who];
    }

    function isB(address who) public view returns (bool) {
        return roles[keccak256("B")][who];
    }

    function eitherRole() external {
        require(isA(msg.sender) || isB(msg.sender), "either");
        value = 7;
    }

    function _msgSender() internal view returns (address) {
        return msg.sender;
    }

    function _checkRole(bytes32 role) internal view {
        _checkRole(role, _msgSender());
    }

    function _checkRole(bytes32 role, address account) internal view {
        if (!hasRole(role, account)) revert("account");
    }

    modifier onlyRoleChecked(bytes32 role) {
        _checkRole(role);
        _;
    }

    function roleViaHelper() external onlyRoleChecked(ADMIN) {
        value = 8;
    }

    function originCheck() external {
        require(tx.origin == owner, "origin");
        value = 9;
    }

    bool public paused;
    mapping(address => bool) public blacklisted;

    function ownerUnpaused() external {
        require(msg.sender == owner && !paused, "owner, unpaused");
        value = 10;
    }

    function roleOrOwnerUnpaused() external {
        if (!((isA(msg.sender) || msg.sender == owner) && !paused)) revert("nested");
        value = 11;
    }

    function notBlacklisted() external {
        require(!blacklisted[msg.sender], "blacklisted");
        value = 12;
    }

    function ifNotRole() external {
        if (!hasRole(ADMIN, msg.sender)) revert("role");
        value = 13;
    }

    bool public open;

    function ownerOrOpen() external {
        require(msg.sender == owner || open, "closed");
        value = 15;
    }

    function unreadable() external {
        require(hasRole(ADMIN, msg.sender) && uint160(msg.sender) > 7, "opaque");
        value = 14;
    }

    function deep() external {
        _d1();
        value = 4;
    }

    function _d1() internal view { _d2(); }
    function _d2() internal view { _d3(); }
    function _d3() internal view { _d4(); }
    function _d4() internal view { _d5(); }
    function _d5() internal view { _d6(); }
    function _d6() internal view { _d7(); }
    function _d7() internal view { _d8(); }
    function _d8() internal view { _d9(); }
    function _d9() internal view { _d10(); }
    function _d10() internal view { _d11(); }
    function _d11() internal view { _d12(); }
    function _d12() internal view { _d13(); }
    function _d13() internal view { _d14(); }
    function _d14() internal view { require(msg.sender == owner, "deep"); }
}

contract Targets {
    address constant KNOWN = 0x000000000000000000000000000000000000dEaD;
    IPool public pool;
    address public poolAddress;
    IPool public immutable fixedPool;
    IPool[] public pools;

    constructor(IPool p) {
        fixedPool = p;
    }

    function fromState() external {
        pool.supply(address(0), 1);
    }

    function fromCastState() external {
        IPool(poolAddress).supply(address(0), 1);
    }

    function fromImmutable() external {
        fixedPool.supply(address(0), 1);
    }

    function fromParam(uint256 amount, IPool p) external {
        p.supply(address(0), amount);
    }

    function fromSender() external {
        IPool(msg.sender).supply(address(0), 1);
    }

    function twice() external {
        pool.supply(address(0), 1);
        pool.supply(address(0), 1);
    }

    function fromExpression(uint256 i) external {
        pools[i].supply(address(0), 1);
    }

    function fromConstant() external {
        IPool(KNOWN).supply(address(0), 1);
    }

    function fromLiteral() external {
        IPool(address(0x1234)).supply(address(0), 1);
    }
}

contract LowLevel {
    function ignored(address to) external {
        to.call("");
    }

    function checked(address to) external {
        (bool ok, ) = to.call("");
        require(ok, "call");
    }

    function unusedOk(address to) external {
        (bool ok, ) = to.call("");
    }

    function sendIgnored(address payable to) external {
        to.send(1);
    }

    function sendUsed(address payable to) external returns (bool) {
        return to.send(1);
    }

    function sendStored(address payable to) external returns (bool sent) {
        bool ok = to.send(1);
        sent = ok;
    }

    function staticIgnored(address to) external view {
        to.staticcall("");
    }

    function delegated(address to) external {
        (bool ok, bytes memory data) = to.delegatecall("");
        require(ok, string(data));
    }
}

contract Builtins {
    uint256 public stampCount;

    modifier stamp(uint256 t) {
        unchecked { stampCount += block.number; }
        _;
    }

    function stamped() external stamp(block.timestamp) {
        stampCount = 1;
    }

    function all(bytes32 h, uint8 v, bytes32 r, bytes32 s, address a) external payable returns (uint256 out) {
        address signer = ecrecover(h, v, r, s);
        out = uint256(uint160(signer)) + uint160(tx.origin);
        out += block.timestamp + block.number + uint256(blockhash(block.number - 1));
        out += a.code.length + gasleft() + msg.value + a.balance + address(this).balance + block.chainid;
        out += abi.encodePacked(a).length;
        unchecked {
            out += 1;
        }
    }

    function kill(address payable to) external {
        selfdestruct(to);
    }

    function none(uint256 x) external pure returns (uint256) {
        return x + 1;
    }
}
