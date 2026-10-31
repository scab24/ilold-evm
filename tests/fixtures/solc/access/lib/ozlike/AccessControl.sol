// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Context} from "./Context.sol";

abstract contract AccessControl is Context {
    mapping(bytes32 => mapping(address => bool)) private _roles;
    mapping(bytes32 => bytes32) private _admins;

    bytes32 public constant DEFAULT_ADMIN_ROLE = 0x00;

    error AccessControlUnauthorizedAccount(address account, bytes32 neededRole);

    modifier onlyRole(bytes32 role) {
        _checkRole(role);
        _;
    }

    function hasRole(bytes32 role, address account) public view virtual returns (bool) {
        return _roles[role][account];
    }

    function getRoleAdmin(bytes32 role) public view virtual returns (bytes32) {
        return _admins[role];
    }

    function grantRole(bytes32 role, address account) public virtual onlyRole(getRoleAdmin(role)) {
        _roles[role][account] = true;
    }

    function _checkRole(bytes32 role) internal view virtual {
        _checkRole(role, _msgSender());
    }

    function _checkRole(bytes32 role, address account) internal view virtual {
        if (!hasRole(role, account)) {
            revert AccessControlUnauthorizedAccount(account, role);
        }
    }
}
