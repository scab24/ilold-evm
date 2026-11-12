// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IERC20, IERC6909} from "./Tokens.sol";

library Address {
    function sendValue(address payable recipient, uint256 amount) internal {
        bool success;
        assembly {
            success := call(gas(), recipient, amount, 0, 0, 0, 0)
        }
        require(success, "send failed");
    }

    function functionCallWithValue(address target, bytes memory data, uint256 value) internal returns (bytes memory) {
        (bool success, bytes memory returndata) = target.call{value: value}(data);
        require(success, "call failed");
        return returndata;
    }
}

library SafeERC20 {
    function safeTransfer(IERC20 token, address to, uint256 value) internal {
        require(token.transfer(to, value), "transfer failed");
    }

    function safeTransferFrom(IERC20 token, address from, address to, uint256 value) internal {
        require(token.transferFrom(from, to, value), "transferFrom failed");
    }
}

library SafeTransferLib {
    function safeTransferETH(address to, uint256 amount) internal {
        assembly {
            if iszero(call(gas(), to, amount, codesize(), 0x00, codesize(), 0x00)) { revert(0, 0) }
        }
    }

    function safeTransfer(address token, address to, uint256 amount) internal {
        assembly {
            mstore(0x14, to)
            mstore(0x34, amount)
            mstore(0x00, 0xa9059cbb000000000000000000000000)
            if iszero(call(gas(), token, 0, 0x10, 0x44, 0x00, 0x20)) { revert(0, 0) }
            mstore(0x34, 0)
        }
    }

    function balanceOf(address token, address account) internal view returns (uint256 amount) {
        assembly {
            mstore(0x14, account)
            mstore(0x00, 0x70a08231000000000000000000000000)
            amount := mul(mload(0x20), and(gt(returndatasize(), 0x1f), staticcall(gas(), token, 0x10, 0x24, 0x20, 0x20)))
        }
    }
}

type Currency is address;

using CurrencyLibrary for Currency global;

library CurrencyLibrary {
    function transfer(Currency currency, address to, uint256 amount) internal {
        if (Currency.unwrap(currency) == address(0)) {
            (bool success,) = to.call{value: amount}("");
            require(success, "native transfer failed");
        } else {
            require(IERC20(Currency.unwrap(currency)).transfer(to, amount), "transfer failed");
        }
    }

    function balanceOfSelf(Currency currency) internal view returns (uint256) {
        if (Currency.unwrap(currency) == address(0)) return address(this).balance;
        return IERC20(Currency.unwrap(currency)).balanceOf(address(this));
    }

    function toId(Currency currency) internal pure returns (uint256) {
        return uint160(Currency.unwrap(currency));
    }
}

interface IPoolManager is IERC6909 {
    function mint(address to, uint256 id, uint256 amount) external;

    function burn(address from, uint256 id, uint256 amount) external;

    function take(Currency currency, address to, uint256 amount) external;

    function sync(Currency currency) external;

    function settle() external payable returns (uint256);
}

library CurrencySettler {
    function settle(Currency currency, IPoolManager manager, address payer, uint256 amount, bool burn) internal {
        if (burn) {
            manager.burn(payer, currency.toId(), amount);
        } else {
            manager.sync(currency);
            currency.transfer(address(manager), amount);
            manager.settle();
        }
    }

    function take(Currency currency, IPoolManager manager, address recipient, uint256 amount, bool claims) internal {
        if (claims) {
            manager.mint(recipient, currency.toId(), amount);
        } else {
            manager.take(currency, recipient, amount);
        }
    }
}
