// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

library Types {
    struct Reserve {
        uint256 index;
        uint256 rate;
        uint40 updatedAt;
    }

    struct UserConfig {
        uint256 data;
    }
}
