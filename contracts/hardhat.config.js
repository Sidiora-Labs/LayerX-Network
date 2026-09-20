require('dotenv').config({path:__dirname+'/.env'})
require("@nomicfoundation/hardhat-toolbox");
require('@openzeppelin/hardhat-upgrades');

/** @type import('hardhat/config').HardhatUserConfig */
module.exports = {
  solidity: {
    version: "0.8.28",
    settings: {
      evmVersion: "prague",
      optimizer: {
        enabled: true,
        runs: 1000,
      },
    },
  },
  mocha: {
    timeout: 100000000,
  },
  paths: {
    sources: "./src", // contracts are in ./src
  },
  networks: {
    paxlocal: {
      url: process.env.PAXEER_LOCAL_EVM_RPC_URL || "http://127.0.0.1:8545",
    }
  },
};
