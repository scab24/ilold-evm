// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {VersionedInitializable} from "@ozlike/VersionedInitializable.sol";

// The base runs `baseSet` unguarded; the derived contract overrides the modifier.
contract KeeperBase {
    address public keeper;
    uint256 public value;

    modifier onlyKeeperish() virtual {
        _;
    }

    function baseSet(uint256 v) external onlyKeeperish {
        value = v;
    }

    function ownerOnly(uint256 v) external {
        require(msg.sender == keeper, "keeper");
        value = v;
    }
}

contract KeeperDerived is KeeperBase {
    modifier onlyKeeperish() override {
        require(msg.sender == keeper, "keeper");
        _;
    }
}

contract Upgradeable is VersionedInitializable {
    uint256 public value;

    function initialize(uint256 v) external initializer {
        value = v;
    }

    function getRevision() internal pure override returns (uint256) {
        return 1;
    }
}

// A modifier without a body has no code to read: its name is all there is.
abstract contract Governed {
    uint256 public value;

    modifier onlyGovernance() virtual;

    function govSet(uint256 v) external onlyGovernance {
        value = v;
    }
}
