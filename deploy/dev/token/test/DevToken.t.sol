// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {DevToken} from "../src/DevToken.sol";

/// Plain asserts, no test library: a failed `require` fails the test.
contract DevTokenTest {
    address constant DEAD = 0x000000000000000000000000000000000000dEaD;

    function test_the_holder_gets_the_supply_and_can_burn() public {
        DevToken token = new DevToken(address(this), 1000);
        require(token.balanceOf(address(this)) == 1000, "supply");
        require(token.transfer(DEAD, 400), "transfer");
        require(token.balanceOf(DEAD) == 400 && token.balanceOf(address(this)) == 600, "moved");
    }

    function test_no_one_moves_more_than_they_hold_or_are_allowed() public {
        DevToken token = new DevToken(address(this), 10);
        (bool ok,) = address(token).call(abi.encodeCall(DevToken.transfer, (DEAD, 11)));
        require(!ok, "overdraft");
        (ok,) = address(token).call(abi.encodeCall(DevToken.transferFrom, (DEAD, address(this), 1)));
        require(!ok, "no allowance");
        require(token.approve(address(this), 5), "approve");
        require(token.transferFrom(address(this), DEAD, 5), "allowed");
        require(token.allowance(address(this), address(this)) == 0, "spent");
    }
}
