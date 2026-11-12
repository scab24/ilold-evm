// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IPinger {
    function ping() external;
    function quote(uint256 amount) external returns (uint256);
}

contract Child {
    uint256 public seed;

    constructor(uint256 s) {
        seed = s;
    }
}

// One try shape per function: what each catch does after a failure.
contract TryCatch {
    error Failed();

    event Quoted(uint256 value);
    event Rejected(string reason);

    IPinger public pinger;
    Child public child;
    uint256 public failures;
    uint256 public value;
    uint256 public count;
    string public lastError;
    bytes public lastData;
    mapping(uint256 => uint256) public panics;
    mapping(address => uint256) public misses;

    modifier pinged() {
        try pinger.ping() {} catch {
            failures += 1;
        }
        _;
    }

    function step() external {
        count += 1;
    }

    function swallow() external {
        try this.step{gas: 100000}() {} catch {}
        value = 1;
    }

    function rethrow() external {
        try pinger.ping() {
            count += 1;
        } catch {
            revert Failed();
        }
    }

    function record() external {
        try pinger.quote(1) returns (uint256 v) {
            value = v;
        } catch Error(string memory reason) {
            failures += 1;
            lastError = reason;
            misses[msg.sender] += 1;
        }
    }

    function clauses(uint256 amount) external returns (uint256) {
        try pinger.quote(amount) returns (uint256 v) {
            emit Quoted(v);
            return v;
        } catch Error(string memory reason) {
            emit Rejected(reason);
        } catch Panic(uint256 code) {
            panics[code] += 1;
        } catch (bytes memory data) {
            lastData = data;
            pinger.ping();
        }
        return 0;
    }

    function bare() external {
        try pinger.quote(2) returns (uint256 v) {
            value = v;
        } catch Error(string memory) {
            failures += 1;
        } catch {
            _note();
        }
    }

    function create(uint256 s) external {
        try new Child(s) returns (Child c) {
            child = c;
        } catch {
            failures += 1;
        }
    }

    function loop(address[] calldata targets) external {
        for (uint256 i = 0; i < targets.length; i++) {
            try IPinger(targets[i]).ping() {} catch {
                continue;
            }
            count += 1;
        }
    }

    function guarded() external pinged {
        value = 2;
    }

    function _note() internal {
        failures += 1;
    }
}
