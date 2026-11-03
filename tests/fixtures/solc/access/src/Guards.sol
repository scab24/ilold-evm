// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Ownable} from "@ozlike/Ownable.sol";
import {AccessControl} from "@ozlike/AccessControl.sol";
import {Pausable} from "@ozlike/Pausable.sol";
import {ReentrancyGuard} from "@ozlike/ReentrancyGuard.sol";

interface IACL {
    function isPoolAdmin(address account) external view returns (bool);
}

interface IProvider {
    function getACL() external view returns (address);
}

library AuthLib {
    error NotKeeper();

    function requireKeeper(address keeper) internal view {
        if (msg.sender != keeper) {
            revert NotKeeper();
        }
    }
}

// One guard shape per function; user entry points use the caller without checking it.
contract Guards is Ownable, AccessControl, Pausable, ReentrancyGuard {
    bytes32 public constant MINTER_ROLE = keccak256("MINTER");

    address public admin;
    address public keeper;
    IProvider public provider;
    bool public ready;
    uint256 public value;
    mapping(address => bool) public authorized;
    mapping(address => uint256) public balances;

    error Unauthorized();
    error NotReady();
    error NotPoolAdmin();
    error ZeroSender();

    constructor(address admin_) Ownable(msg.sender) {
        admin = admin_;
    }

    modifier onlyAdminRequire() {
        require(msg.sender == admin, "not admin");
        _;
    }

    modifier onlyAdminIf() {
        if (msg.sender != admin) revert Unauthorized();
        _;
    }

    modifier onlyPoolAdmin() {
        _onlyPoolAdmin();
        _;
    }

    // sounds like access control, checks a flag
    modifier onlyWhenReady() {
        if (!ready) revert NotReady();
        _;
    }

    function _onlyPoolAdmin() internal view {
        require(IACL(provider.getACL()).isPoolAdmin(msg.sender), NotPoolAdmin());
    }

    function setByRequire(uint256 v) external onlyAdminRequire {
        value = v;
    }

    function setByIf(uint256 v) external onlyAdminIf {
        value = v;
    }

    function setByOwner(uint256 v) external onlyOwner {
        value = v;
    }

    function setByRole(uint256 v) external onlyRole(MINTER_ROLE) {
        value = v;
    }

    function setByAcl(uint256 v) external onlyPoolAdmin {
        value = v;
    }

    function setByOrigin(uint256 v) external {
        require(tx.origin == admin);
        value = v;
    }

    function setInBody(uint256 v) external {
        if (msg.sender != admin) {
            revert Unauthorized();
        }
        value = v;
    }

    function setByAllowList(uint256 v) external {
        address sender = _msgSender();
        require(authorized[sender], "not authorized");
        value = v;
    }

    function setByKeeper(uint256 v) external {
        AuthLib.requireKeeper(keeper);
        value = v;
    }

    function setWhenReady(uint256 v) external onlyWhenReady {
        value = v;
    }

    function pausedSet(uint256 v) external whenNotPaused {
        value = v;
    }

    function guardedSet(uint256 v) external nonReentrant {
        value = v;
    }

    // guarded by the modifier of the public function it calls
    function addMinter(address account) external {
        grantRole(MINTER_ROLE, account);
    }

    function setByProvider(uint256 v) external {
        require(_msgSender() == address(provider), "not provider");
        value = v;
    }

    function setWithMessage(uint256 v) external {
        if (msg.sender != admin) {
            revert(string(abi.encodePacked("caller ", msg.sender, " is not the admin of this contract")));
        }
        value = v;
    }

    function deposit(uint256 amount) external {
        require(amount > 0, "zero");
        _credit(_msgSender(), amount);
    }

    function withdraw(uint256 amount) external {
        require(balances[msg.sender] >= amount, "balance");
        bool free = IACL(provider.getACL()).isPoolAdmin(msg.sender);
        balances[msg.sender] -= free ? amount : amount + 1;
    }

    function _credit(address from, uint256 amount) internal {
        if (from == address(0)) revert ZeroSender();
        balances[from] += amount;
    }
}
