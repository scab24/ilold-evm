// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {ICurve, IDen, IHook, IToken} from "./Interfaces.sol";
import {Payout} from "./Payout.sol";

abstract contract CurveBase is ICurve {
    IHook public hook;
    IToken public token;
    IDen public den;

    constructor(IHook hook_, IToken token_, IDen den_) {
        hook = hook_;
        token = token_;
        den = den_;
    }

    function migrate() public {
        _graduate();
    }

    function price() external pure returns (uint256) {
        return 1;
    }

    function _graduate() internal {
        hook.migrate{value: address(this).balance}(hook.fee());
    }
}

contract Curve is CurveBase {
    constructor(IHook hook_, IToken token_, IDen den_) CurveBase(hook_, token_, den_) {}

    function buy(address to) external payable {
        token.mint(to, msg.value);
        den.deliver(to);
        if (msg.value > 1 ether) {
            migrate();
        }
        payable(to).transfer(1);
        Payout.pay(token, to, 1);
    }
}
