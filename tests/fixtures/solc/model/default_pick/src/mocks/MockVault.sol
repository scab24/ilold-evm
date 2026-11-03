// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Vault} from "../Vault.sol";

abstract contract Extra {}

// More ancestors than Vault, yet a mock.
contract Faucet is Vault, Extra {}
