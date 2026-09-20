#!/bin/bash

set -e

cd contracts
npm ci
npx hardhat test --network paxlocal test/PaxSoloTest.js
npx hardhat test --network paxlocal test/SetCodeTxTest.js
npx hardhat test --network paxlocal test/TransientStorageTest.js
