pragma solidity ^0.8.0;

abstract contract AbstractContract {
    // Declaring functions
    function getStr(
      string memory _strIn) public view virtual returns(
      string memory);
    function setValue(uint _in1, uint _in2) public virtual;
    function add() public virtual returns(uint);
}