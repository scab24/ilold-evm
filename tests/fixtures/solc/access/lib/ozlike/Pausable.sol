// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

abstract contract Pausable {
    bool private _paused;

    error EnforcedPause();

    modifier whenNotPaused() {
        _requireNotPaused();
        _;
    }

    function paused() public view virtual returns (bool) {
        return _paused;
    }

    function _requireNotPaused() internal view virtual {
        if (paused()) {
            revert EnforcedPause();
        }
    }
}
