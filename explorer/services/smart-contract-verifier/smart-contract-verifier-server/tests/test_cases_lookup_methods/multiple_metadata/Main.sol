// SPDX-License-Identifier: GPL-3.0
pragma solidity =0.8.7;

contract A {
    function a() public pure returns (bytes memory) {
        return "";
    }
}

contract B {
    bytes code;

    constructor() {
        code = type(A).creationCode;
    }

    function a() public pure returns (bytes memory) {
        return type(A).creationCode;
    }
}