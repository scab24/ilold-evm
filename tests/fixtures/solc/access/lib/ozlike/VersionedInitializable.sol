// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// Shaped like Aave's VersionedInitializable.
abstract contract VersionedInitializable {
    uint256 private lastInitializedRevision = 0;
    bool private initializing;

    modifier initializer() {
        uint256 revision = getRevision();
        require(initializing || revision > lastInitializedRevision, "already initialized");
        bool isTopLevelCall = !initializing;
        if (isTopLevelCall) {
            initializing = true;
            lastInitializedRevision = revision;
        }
        _;
        if (isTopLevelCall) {
            initializing = false;
        }
    }

    function getRevision() internal pure virtual returns (uint256);
}
