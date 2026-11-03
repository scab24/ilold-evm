// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

// Dependency library that the project only calls; nothing inherits it.
library ExtCounter {
    struct Counter {
        uint256 value;
    }

    error Empty();

    function increment(Counter storage c) internal {
        c.value += 1;
    }

    function reset(Counter storage c) public {
        if (c.value == 0) revert Empty();
        c.value = 0;
    }

    // called only from a modifier override that no own function of its contract uses
    function decrement(Counter storage c) internal {
        c.value -= 1;
    }
}
