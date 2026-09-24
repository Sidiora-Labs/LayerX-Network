/* eslint-disable max-len */
import type { SmartContract } from 'types/api/contract';

export const verified: SmartContract = {
  abi: [ { anonymous: false, inputs: [ { indexed: true, internalType: 'address', name: 'src', type: 'address' }, { indexed: true, internalType: 'address', name: 'guy', type: 'address' }, { indexed: false, internalType: 'uint256', name: 'wad', type: 'uint256' } ], name: 'Approval', type: 'event' } ],
  can_be_visualized_via_sol2uml: true,
  compiler_version: 'v0.5.16+commit.9c3226ce',
  constructor_args: 'constructor_args',
  creation_bytecode: 'creation_bytecode',
  creation_status: 'success',
  deployed_bytecode: 'deployed_bytecode',
  compiler_settings: {
    evmVersion: 'london',
    remappings: [
      '@openzeppelin/=node_modules/@openzeppelin/',
    ],
  },
  evm_version: 'default',
  is_verified: true,
  is_blueprint: false,
  name: 'WPOA',
  optimization_enabled: true,
  optimization_runs: 1500,
  source_code: 'source_code',
  verified_at: '2021-08-03T10:40:41.679421Z',
  decoded_constructor_args: [
    [ '0xc59615da2da226613b1c78f0c6676cac497910bc', { internalType: 'address', name: '_token', type: 'address' } ],
    [ [ 1800, 3600, 7200 ], { internalType: 'uint256[]', name: '_durations', type: 'uint256[]' } ],
    [ '900000000', { internalType: 'uint256', name: '_totalSupply', type: 'uint256' } ],
  ],
  external_libraries: [
    { address_hash: '0xa62744BeE8646e237441CDbfdedD3458861748A8', name: 'Sol' },
    { address_hash: '0xa62744BeE8646e237441CDbfdedD3458861748A8', name: 'math' },
  ],
  language: 'solidity',
  license_type: 'gnu_gpl_v3',
  is_verified_via_eth_bytecode_db: null,
  is_changed_bytecode: null,
  is_verified_via_sourcify: null,
  is_fully_verified: null,
  is_partially_verified: null,
  sourcify_repo_url: null,
  file_path: '',
  additional_sources: [],
  verified_twin_address_hash: null,
  conflicting_implementations: null,
};

export const certified: SmartContract = {
  ...verified,
  certified: true,
};

export const withMultiplePaths: SmartContract = {
  ...verified,
  file_path: './simple_storage.sol',
  additional_sources: [
    {
      file_path: '/contracts/protocol/libraries/logic/GenericLogic.sol',
      source_code: '// SPDX-License-Identifier: GPL-3.0 \n pragma solidity >=0.7.0 <0.9.0;      \n           contract Storage {\n //2112313123; \nuint256 number; \n function store(uint256 num) public {\nnumber = num;\n}\n function retrieve() public view returns (uint256)\n {\nreturn number;\n}\n}',
    },
  ],
};

export const verifiedViaSourcify: SmartContract = {
  ...verified,
  is_verified_via_sourcify: true,
  is_fully_verified: false,
  is_partially_verified: true,
  sourcify_repo_url: 'https://repo.sourcify.dev/contracts//full_match/99/0x51891596E158b2857e5356DC847e2D15dFbCF2d0/',
};

export const verifiedViaEthBytecodeDb: SmartContract = {
  ...verified,
  is_verified_via_eth_bytecode_db: true,
};

export const withTwinAddress: SmartContract = {
  ...verified,
  is_verified: false,
  verified_twin_address_hash: '0xa62744bee8646e237441cdbfdedd3458861748a8',
};

export const withProxyAddress: SmartContract = {
  ...verified,
  is_verified: false,
  verified_twin_address_hash: '0xa62744bee8646e237441cdbfdedd3458861748a8',
};

export const selfDestructed: SmartContract = {
  ...verified,
  creation_status: 'selfdestructed',
};

export const withChangedByteCode: SmartContract = {
  ...verified,
  is_changed_bytecode: true,
  is_blueprint: true,
};

export const zkSync: SmartContract = {
  ...verified,
  zk_compiler_version: 'v1.2.5',
  optimization_enabled: true,
  optimization_runs: 's',
};

export const stylusRust: SmartContract = {
  ...verified,
  language: 'stylus_rust',
  github_repository_metadata: {
    commit: 'af5029f822815e32def0015bf8e591e769c62f34',
    path_prefix: 'examples/erc20',
    repository_url: 'https://github.com/blockscout/cargo-stylus-test-examples',
  },
  compiler_version: 'v0.5.6',
  package_name: 'erc20',
  evm_version: null,
};

export const nonVerified: SmartContract = {
  is_verified: false,
  is_blueprint: false,
  creation_bytecode: 'creation_bytecode',
  deployed_bytecode: 'deployed_bytecode',
  creation_status: 'success',
  abi: null,
  compiler_version: null,
  evm_version: null,
  optimization_enabled: null,
  optimization_runs: null,
  name: null,
  verified_at: null,
  is_verified_via_eth_bytecode_db: null,
  is_changed_bytecode: null,
  is_verified_via_sourcify: null,
  is_fully_verified: null,
  is_partially_verified: null,
  sourcify_repo_url: null,
  source_code: null,
  constructor_args: null,
  decoded_constructor_args: null,
  can_be_visualized_via_sol2uml: null,
  file_path: '',
  additional_sources: [],
  external_libraries: null,
  verified_twin_address_hash: null,
  conflicting_implementations: null,
  language: null,
  license_type: null,
};
