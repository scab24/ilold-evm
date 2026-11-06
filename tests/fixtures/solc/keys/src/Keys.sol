// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

contract Keys {
    mapping(address => uint256) public balances;
    mapping(address => mapping(address => uint256)) public allowance;
    mapping(uint256 => uint256) public slots;
    mapping(uint256 => Position) internal positions;
    address public treasury;
    uint256 public total;

    struct Position {
        uint256 amount;
    }

    modifier funded(address who) {
        require(balances[who] > 0, "empty");
        _;
    }

    function deposit() external payable {
        balances[msg.sender] += msg.value;
    }

    function credit(address account, uint256 amount) external {
        balances[account] = amount;
    }

    function fundTreasury(uint256 amount) external {
        balances[treasury] = amount;
    }

    function approve(address owner, address spender, uint256 amount) external {
        allowance[owner][spender] = amount;
    }

    function store(uint256 seed, uint256 value) external {
        slots[slotOf(seed)] = value;
    }

    function selfCredit(uint256 amount) external {
        balances[address(this)] = amount;
    }

    function setFirst(uint256 value) external {
        slots[0] = value;
    }

    function setNext(uint256 value) external {
        uint256 next = value + 1;
        slots[next] = value;
    }

    function move(address from, address to, uint256 amount) external {
        balances[from] -= amount;
        balances[to] += amount;
    }

    function creditLater(address account, uint256 amount) external {
        _credit(account, amount);
        total += amount;
    }

    function withdraw(address account, uint256 amount) external funded(account) {
        balances[account] -= amount;
    }

    function grow(uint256 key, uint256 amount) external {
        _position(key).amount += amount;
    }

    function shadowed(uint256 i) external {
        slots[i] = 1;
        for (uint256 j = 0; j < 1; j++) {
            uint256 i = j;
            slots[i] = 2;
        }
    }

    function _position(uint256 id) internal view returns (Position storage) {
        return positions[id];
    }

    function _credit(address account, uint256 amount) internal {
        balances[account] += amount;
    }

    function slotOf(uint256 seed) internal pure returns (uint256) {
        return seed * 2;
    }
}
