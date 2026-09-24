// SPDX-License-Identifier: GPL-3.0
pragma solidity =0.8.14;

contract A {
}

contract B {
    bytes code;
    constructor() {
        code = type(A).creationCode;
    }
}