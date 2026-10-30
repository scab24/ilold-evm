// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IToken {
    function transfer(address to, uint256 amount) external returns (bool);

    function balanceOf(address who) external view returns (uint256);
}

contract Child {
    uint256 public x;

    constructor(uint256 v) {
        x = v;
    }
}

contract Caller {
    IToken public token;

    function viaCast(address t, address to) external {
        IToken(t).transfer(to, 1);
    }

    function viaNestedCast(uint160 raw) external view returns (uint256) {
        return IToken(address(raw)).balanceOf(address(this));
    }

    function viaState() external view returns (uint256) {
        return token.balanceOf(msg.sender);
    }

    function lowLevel(address target, bytes calldata data) external payable {
        (bool ok, ) = target.call{value: msg.value, gas: 5000}(data);
        require(ok, "call");
        (ok, ) = address(target).delegatecall(data);
        (ok, ) = target.staticcall(data);
    }

    function pay(address payable to, address other) external {
        to.transfer(1);
        bool sent = payable(other).send(1);
        require(sent, "send");
    }

    function create(bytes32 salt) external returns (address a, address b) {
        a = address(new Child(1));
        b = address(new Child{salt: salt}(2));
    }

    function builtins(bytes calldata data, bytes32 h, uint8 v) external view returns (bytes32) {
        bytes32 k = keccak256(abi.encodePacked(data, abi.encode(h)));
        k = sha256(bytes.concat(k));
        address signer = ecrecover(h, v, h, h);
        uint256 m = addmod(1, 2, 3) + mulmod(1, 2, 3) + gasleft();
        bytes4 id = type(IToken).interfaceId;
        string memory s = string.concat("a", "b");
        require(signer != address(0) || m > 0 || id != 0 || bytes(s).length > 0, "x");
        assert(blockhash(block.number - 1) != h || m > 0);
        uint256 d = abi.decode(data, (uint256));
        if (d == 0) revert("zero");
        return keccak256(abi.encodeWithSelector(IToken.transfer.selector, signer, d)) ^ k;
    }
}
