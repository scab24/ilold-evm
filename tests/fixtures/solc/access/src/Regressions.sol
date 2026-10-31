// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IToken {
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function isMember(address account) external view returns (bool);
}

// Checks that look like access control but do not restrict who may call.
contract Lookalikes {
    address public admin;
    bool public open;
    uint256 public value;
    IToken public token;
    mapping(address => bool) public blocked;

    error Closed();

    // the admin check runs only when the early return is not taken
    function setAfterEarlyReturn(uint256 v) external {
        if (open) {
            value = v;
            return;
        }
        require(msg.sender == admin, "admin");
        value = v;
    }

    // a state-changing call that takes the caller is not a role check
    function payIn(uint256 amount) external {
        require(token.transferFrom(msg.sender, address(this), amount), "transfer");
        value += amount;
    }

    // an external view call that takes the caller is
    function membersOnly(uint256 v) external {
        require(token.isMember(msg.sender), "member");
        value = v;
    }

    // everyone except the admin
    function everyoneButAdmin(uint256 v) external {
        require(msg.sender != admin, "admin");
        value = v;
    }

    function adminReverts(uint256 v) external {
        if (msg.sender == admin) revert Closed();
        value = v;
    }

    function blockList(uint256 v) external {
        if (blocked[msg.sender]) revert Closed();
        value = v;
    }

    // anyone while open
    function adminOrOpen(uint256 v) external {
        require(msg.sender == admin || open, "closed");
        value = v;
    }

    function notAdminAndClosed(uint256 v) external {
        if (msg.sender != admin && !open) revert Closed();
        value = v;
    }

    // both must hold: still an admin check
    function openAndAdmin(uint256 v) external {
        require(open && msg.sender == admin, "closed");
        value = v;
    }

    function adminOrOwner(uint256 v) external {
        if (!(msg.sender == admin || tx.origin == admin)) revert Closed();
        value = v;
    }
}

// Overloads: one guarded, one not.
contract Overloads {
    address public admin;
    uint256 public value;

    function put(uint256 v) external {
        value = v;
    }

    function put(address a) external {
        require(msg.sender == admin, "admin");
        admin = a;
    }
}

// A single `put`, guarded: its key is the bare name.
contract SinglePut {
    address public admin;
    uint256 public value;

    function put(uint256 v) external {
        require(msg.sender == admin, "admin");
        value = v;
    }
}
