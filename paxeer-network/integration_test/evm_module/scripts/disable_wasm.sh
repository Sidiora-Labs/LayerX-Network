#!/bin/bash

set -e

cd contracts
npm ci
npx hardhat test --network paxlocal test/DisableWasmTest.js