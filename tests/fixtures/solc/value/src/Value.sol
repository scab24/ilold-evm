// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IBurnable, IERC20, IERC1155, IMintableNft, IMintableToken} from "@vendor/Tokens.sol";
import {Address, Currency, CurrencySettler, IPoolManager, SafeERC20, SafeTransferLib} from "@vendor/Libraries.sol";

contract Ledger {
    mapping(bytes32 => address) public owners;

    function transfer(bytes32 key, address to) external {
        owners[key] = to;
    }
}

contract Payments {
    using SafeERC20 for IERC20;
    using CurrencySettler for Currency;

    IERC20 public asset;
    IMintableToken public share;
    IMintableNft public nft;
    IERC1155 public multi;
    IPoolManager public manager;
    IBurnable public burnable;
    Ledger public ledger;
    Currency public quote;
    address public treasury;

    modifier refundExcess(uint256 price) {
        _;
        payable(msg.sender).transfer(msg.value - price);
    }

    function payCall(address to, uint256 amount) external {
        (bool ok,) = to.call{value: amount}("");
        require(ok, "call");
    }

    function payTransfer(address payable to) external {
        to.transfer(1 ether);
    }

    function paySend(address payable to) external returns (bool) {
        return to.send(1 ether);
    }

    function buy(uint256 price) external payable refundExcess(price) {
        share.mint(msg.sender, price);
    }

    function deposit(uint256 amount) external {
        asset.transferFrom(msg.sender, address(this), amount);
    }

    function withdraw(uint256 amount) external {
        asset.transfer(msg.sender, amount);
    }

    function burnShares(uint256 amount) external {
        share.burn(amount);
        share.burnFrom(msg.sender, amount);
        burnable.burn(amount);
    }

    function moveNft(address to, uint256 id) external {
        nft.safeTransferFrom(address(this), to, id);
        nft.transferFrom(msg.sender, address(this), id);
        nft.mint(to, id);
    }

    function moveMulti(address to, uint256 id, uint256 amount) external {
        multi.safeTransferFrom(address(this), to, id, amount, "");
    }

    function moveClaims(address to, uint256 id, uint256 amount) external {
        manager.transfer(to, id, amount);
        manager.transferFrom(msg.sender, to, id, amount);
        manager.mint(address(this), id, amount);
        manager.burn(address(this), id, amount);
    }

    function safePay(address to, uint256 amount) external {
        asset.safeTransfer(to, amount);
        SafeERC20.safeTransferFrom(asset, msg.sender, to, amount);
    }

    function sendEth(address payable to, uint256 amount) external {
        Address.sendValue(to, amount);
    }

    function callWithValue(address target, bytes calldata data) external payable {
        Address.functionCallWithValue(target, data, msg.value);
    }

    function soladyPay(address to, uint256 amount) external {
        SafeTransferLib.safeTransferETH(to, amount);
        SafeTransferLib.safeTransfer(address(asset), to, amount);
    }

    function settleQuote(uint256 amount, bool claims) external {
        quote.settle(manager, address(this), amount, false);
        quote.settle(manager, address(this), amount, true);
        quote.take(manager, msg.sender, amount, claims);
        quote.take(manager, address(this), amount, true);
        quote.transfer(treasury, amount);
    }

    function balances(address who, uint256 id) external view returns (uint256 total) {
        total = address(this).balance + who.balance + asset.balanceOf(who) + nft.balanceOf(who);
        total += multi.balanceOf(who, id) + manager.balanceOf(who, id);
        total += SafeTransferLib.balanceOf(address(asset), who) + quote.balanceOfSelf();
    }

    function unrecognized(address target, bytes calldata data, bytes32 key) external {
        ledger.transfer(key, msg.sender);
        (bool ok,) = target.call(data);
        require(ok, "call");
    }

    receive() external payable {}
}
