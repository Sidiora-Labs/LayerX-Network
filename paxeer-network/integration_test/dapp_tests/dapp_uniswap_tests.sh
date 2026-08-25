#!/bin/bash

set -e

cd contracts
npm ci

cd ../integration_test/dapp_tests
npm ci
npx hardhat compile

export DAPP_TEST_ENV=paxlocal
npx hardhat test --network paxlocal uniswap/uniswapTest.js
