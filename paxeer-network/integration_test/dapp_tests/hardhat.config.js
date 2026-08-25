require("@nomiclabs/hardhat-waffle");
require("@nomiclabs/hardhat-ethers");

const mnemonic = process.env.DAPP_TESTS_MNEMONIC;
const accounts = mnemonic
  ? { mnemonic, path: "m/44'/118'/0'/0/0", initialIndex: 0, count: 1 }
  : undefined;

function remoteNetwork(url) {
  return url && accounts ? { url, accounts } : undefined;
}

/** @type import('hardhat/config').HardhatUserConfig */
module.exports = {
  solidity: {
    version: "0.8.20",
    settings: {
      optimizer: {
        enabled: true,
        runs: 1000,
      },
    },
  },
  mocha: {
    timeout: 100000000,
  },
  networks: {
    paxlocal: {
      url: process.env.PAXEER_LOCAL_EVM_RPC_URL || "http://127.0.0.1:8545",
      ...(accounts ? { accounts } : {}),
    },
    ...(remoteNetwork(process.env.PAXEER_TESTNET_EVM_RPC_URL)
      ? { testnet: remoteNetwork(process.env.PAXEER_TESTNET_EVM_RPC_URL) }
      : {}),
    ...(remoteNetwork(process.env.PAXEER_DEVNET_EVM_RPC_URL)
      ? { devnet: remoteNetwork(process.env.PAXEER_DEVNET_EVM_RPC_URL) }
      : {}),
  },
};
