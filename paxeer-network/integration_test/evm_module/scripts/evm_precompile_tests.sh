#!/bin/bash

set -e

cd contracts
npm ci
npx hardhat test --network paxlocal test/EVMPrecompileTest.js
npx hardhat test --network paxlocal test/PaxEndpointsTest.js
npx hardhat test --network paxlocal test/AssociateTest.js
