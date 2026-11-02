// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Ownable} from "dep/Ownable.sol";
import {MathLib, IFeed} from "dep/MathLib.sol";

event Swept(address indexed to);

function clamp(uint256 x) pure returns (uint256) {
    return x > 100 ? 100 : x;
}

// A modifier without code: what it checks is unknown.
abstract contract Gate {
    modifier gated() virtual;
}

// Every record kind of the graph export.
contract Vault is Ownable, Gate {
    using MathLib for uint256;

    event Deposited(address indexed who, uint256 amount);
    event Deposited(address indexed who);

    error TooSmall(uint256 amount);

    uint256 public total;
    mapping(address => uint256) public balances;
    IFeed public feed;
    uint256[] internal history;

    constructor(IFeed initial) {
        feed = initial;
        _setOwner(msg.sender);
    }

    modifier gated() override {
        _;
    }

    function deposit(uint256 amount) external {
        if (amount == 0) revert TooSmall(amount);
        balances[msg.sender] += amount.twice();
        _bump(clamp(amount));
        emit Deposited(msg.sender, amount);
    }

    function ping() external gated {
        emit Deposited(msg.sender);
    }

    function touch() external {
        _announce();
    }

    function _announce() internal {
        emit Deposited(msg.sender);
        emit Swept(msg.sender);
    }

    function price() external view returns (uint256) {
        return feed.price();
    }

    function setFeed(IFeed next) external onlyOwner {
        feed = next;
    }

    // a storage argument would match both overloads
    function sum() external view returns (uint256) {
        uint256[] memory copy = history;
        return _sum(copy);
    }

    function spawn(uint256 amount) external returns (Receipt) {
        return new Receipt{salt: bytes32(amount)}(amount);
    }

    function _bump(uint256 x) internal {
        total += x;
        history.push(x);
    }

    function _sum(uint256[] storage xs) internal view returns (uint256 s) {
        for (uint256 i; i < xs.length; i++) {
            s += xs[i];
        }
    }

    function _sum(uint256[] memory xs) internal pure returns (uint256 s) {
        for (uint256 i; i < xs.length; i++) {
            s += xs[i];
        }
    }
}

contract Receipt {
    uint256 public amount;

    constructor(uint256 value) {
        amount = value;
    }
}

abstract contract Pending {
    modifier pending() virtual;

    uint256 internal seen;

    function mark() external pending {
        seen += 1;
    }
}
