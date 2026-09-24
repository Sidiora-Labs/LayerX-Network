import type { SmartContractMethodRead, SmartContractMethodWrite } from 'ui/address/contract/methods/types';

export const read: Array<SmartContractMethodRead> = [
  {
    constant: true,
    inputs: [
      { internalType: 'address', name: 'wallet', type: 'address' },
    ],
    method_id: '70a08231',
    name: 'FLASHLOAN_PREMIUM_TOTAL',
    outputs: [
      { internalType: 'uint256', name: 'amount', type: 'uint256' },
    ],
    payable: false,
    stateMutability: 'view',
    type: 'function',
  },
  {
    constant: true,
    inputs: [],
    method_id: '06fdde03',
    name: 'name',
    outputs: [
      { internalType: 'string', name: '', type: 'string' },
    ],
    payable: false,
    stateMutability: 'view',
    type: 'function',
  },
];

export const write: Array<SmartContractMethodWrite> = [
  {
    payable: true,
    stateMutability: 'payable',
    type: 'fallback',
    inputs: [
      { internalType: 'bytes', name: 'input', type: 'bytes' },
    ],
    outputs: [
      { internalType: 'bytes', name: 'output', type: 'bytes' },
    ],
  },
  {
    constant: false,
    inputs: [
      { internalType: 'address', name: 'guy', type: 'address' },
      { internalType: 'uint256', name: 'wad', type: 'uint256' },
    ],
    name: 'setReserveInterestRateStrategyAddress',
    outputs: [
      { internalType: 'bool', name: '', type: 'bool' },
    ],
    payable: false,
    stateMutability: 'nonpayable',
    type: 'function',
    method_id: '0x01',
  },
  {
    constant: false,
    inputs: [
      { internalType: 'address', name: 'src', type: 'address' },
      { internalType: 'address', name: 'dst', type: 'address' },
    ],
    name: 'transferFrom',
    outputs: [
      { internalType: 'bool', name: '', type: 'bool' },
    ],
    payable: true,
    stateMutability: 'payable',
    type: 'function',
    method_id: '0x02',
  },
  {
    stateMutability: 'payable',
    type: 'receive',
  },
  {
    constant: false,
    inputs: [],
    name: 'pause',
    outputs: [],
    payable: false,
    stateMutability: 'nonpayable',
    type: 'function',
    method_id: '0x03',
  },
  {
    constant: false,
    inputs: [
      { name: '_from', type: 'address' },
      { name: '_to', type: 'address' },
      { name: '_tokenId', type: 'uint256' },
      { name: '_data', type: 'bytes' },
    ],
    name: 'safeTransferFrom',
    outputs: [],
    payable: false,
    stateMutability: 'nonpayable',
    type: 'function',
    method_id: '0x04',
  },
  {
    constant: false,
    inputs: [
      { name: '_tokenId', type: 'uint256' },
      { name: '_hash', type: 'bytes32' },
      { name: '_keepRequestToken', type: 'bool' },
      { name: '_newOwner', type: 'address' },
      { name: '_signature', type: 'bytes' },
    ],
    name: 'requestToken',
    outputs: [
      { name: 'reward', type: 'uint256' },
    ],
    payable: false,
    stateMutability: 'nonpayable',
    type: 'function',
    method_id: '0x05',
  },
  {
    constant: false,
    inputs: [
      { name: '_tokenId', type: 'uint256' },
      { name: '_imprint', type: 'bytes32' },
      { name: '_uri', type: 'string' },
      { name: '_initialKey', type: 'address' },
      { name: '_tokenRecoveryTimestamp', type: 'uint256' },
      { name: '_initialKeyIsRequestKey', type: 'bool' },
    ],
    name: 'hydrateToken',
    outputs: [
      { name: '', type: 'uint256' },
    ],
    payable: false,
    stateMutability: 'nonpayable',
    type: 'function',
    is_invalid: true,
  },
];
