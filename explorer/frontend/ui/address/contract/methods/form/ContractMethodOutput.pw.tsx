import React from 'react';
import type { AbiFunction } from 'viem';

import { test, expect } from 'playwright/lib';

import ContractMethodOutput from './ContractMethodOutput';

const abiItem: AbiFunction = {
  inputs: [],
  name: 'testAbiMethod',
  stateMutability: 'view',
  type: 'function',
  outputs: [
    // PRIMITIVES
    { internalType: 'address', name: 'owner', type: 'address' },
    { internalType: 'uint256', name: 'validatorsCount', type: 'uint256' },
    { internalType: 'bool', name: '', type: 'bool' },

    // ARRAY OF PRIMITIVES
    { internalType: 'address[]', name: 'interChainClients', type: 'address[]' },

    // NESTED ARRAY
    { internalType: 'uint256[2][]', name: 'chainIds', type: 'uint256[2][]' },

    // TUPLE
    {
      components: [
        {
          components: [
            { internalType: 'bool', name: 'executed', type: 'bool' },
            { internalType: 'uint56', name: 'snapshotId', type: 'uint56' },
            { internalType: 'string', name: 'descriptionURL', type: 'string' },
          ],
          internalType: 'struct IGovValidators.ProposalCore',
          name: 'core',
          type: 'tuple',
        },
      ],
      internalType: 'struct IGovValidators.ExternalProposal',
      name: '',
      type: 'tuple',
    },

    // ARRAY OF TUPLE
    {
      components: [
        {
          components: [
            { internalType: 'enum IGovValidators.ProposalType', name: 'proposalType', type: 'uint8' },
            { internalType: 'bytes', name: 'data', type: 'bytes' },
            {
              components: [
                { internalType: 'bool', name: 'executed', type: 'bool' },
                { internalType: 'uint56', name: 'snapshotId', type: 'uint56' },
              ],
              internalType: 'struct IGovValidators.ProposalCore',
              name: 'core',
              type: 'tuple',
            },
          ],
          internalType: 'struct IGovValidators.InternalProposal',
          name: 'proposal',
          type: 'tuple',
        },
        { internalType: 'enum IGovValidators.ProposalState', name: 'proposalState', type: 'uint8' },
      ],
      internalType: 'struct IGovValidators.InternalProposalView[]',
      name: 'internalProposals',
      type: 'tuple[]',
    },

    // ARRAY OF TUPLES WITHOUT NAMES
    {
      components: [
        { type: 'address' },
        { type: 'uint256' },
      ],
      internalType: 'struct SharingPercentage[]',
      name: '_sharingPercentages',
      type: 'tuple[]',
    },
  ],
};

const result = [
  '0x0000000000000000000000000000000000000000',
  BigInt(42),
  false,
  [
    '0x92a309C640c3f6AF4F84FE40120fD02b58E3Aa96',
    '0x588c7Bda9366EEf83EdF67049a1C45f737aFFe0F',
  ],
  [
    [ BigInt(11_000), BigInt(12_000), BigInt(13_000) ],
    [ BigInt(21_000), BigInt(22_000) ],
  ],
  {
    core: {
      executed: true,
      snapshotId: BigInt(77),
      descriptionURL: '',
    },
  },
  [
    {
      proposalState: 1,
      proposal: {
        proposalType: 100,
        data: '0x000100',
        core: {
          executed: true,
          snapshotId: 111,
        },
      },
    },
    {
      proposalState: 3,
      proposal: {
        proposalType: 300,
        data: '0x000300',
        core: {
          executed: true,
          snapshotId: 333,
        },
      },
    },
  ],
  [
    [ '0xfD36176C63dA52E783a347DE3544B0b44C7054a6', 0 ],
    [ '0xC9534cB913150aD3e98D792857689B55e2404212', 3500 ],
  ],
];

const onSettle = () => {};

test('preview mode', async({ render }) => {
  const component = await render(
    <ContractMethodOutput
      abiItem={ abiItem }
      data={ undefined }
      mode="preview"
      onSettle={ onSettle }
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('result mode', async({ render }) => {
  const component = await render(
    <ContractMethodOutput
      abiItem={ abiItem }
      data={ result }
      mode="result"
      onSettle={ onSettle }
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('single output', async({ render }) => {
  const component = await render(
    <ContractMethodOutput
      abiItem={{ ...abiItem, outputs: abiItem.outputs.slice(3, 4) }}
      data={ result[3] }
      mode="result"
      onSettle={ onSettle }
    />,
  );
  await expect(component).toHaveScreenshot();
});
