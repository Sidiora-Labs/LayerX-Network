import React from 'react';

import type { FormSubmitHandler, SmartContractMethod } from '../types';

import { test, expect } from 'playwright/lib';

import ContractMethodForm from './ContractMethodForm';

const onSubmit: FormSubmitHandler = () => Promise.resolve({ source: 'wallet_client' as const, data: { hash: '0x0000' as `0x${ string }` } });
const onReset = () => {};

const data: SmartContractMethod = {
  inputs: [
    // TUPLE
    {
      components: [
        { internalType: 'address', name: 'offerToken', type: 'address' },
        { internalType: 'uint256', name: 'offerIdentifier', type: 'uint256' },
        { internalType: 'enum BasicOrderType', name: 'basicOrderType', type: 'uint8' },
        {
          components: [
            { internalType: 'uint256', name: 'amount', type: 'uint256' },
            { internalType: 'address payable', name: 'recipient', type: 'address' },
          ],
          internalType: 'struct AdditionalRecipient[]',
          name: 'additionalRecipients',
          type: 'tuple[]',
        },
        { internalType: 'bytes', name: 'signature', type: 'bytes' },
      ],
      internalType: 'struct BasicOrderParameters',
      name: 'parameters',
      type: 'tuple',
    },

    // NESTED ARRAY OF TUPLES
    {
      components: [
        {
          internalType: 'uint256',
          name: 'orderIndex',
          type: 'uint256',
        },
        {
          internalType: 'uint256',
          name: 'itemIndex',
          type: 'uint256',
        },
      ],
      internalType: 'struct FulfillmentComponent[][]',
      name: '',
      type: 'tuple[][]',
    },

    // TOP LEVEL NESTED ARRAY
    {
      internalType: 'int256[2][][3]',
      name: 'ParentArray',
      type: 'int256[2][][3]',
    },

    // LITERALS
    { internalType: 'bytes32', name: 'fulfillerConduitKey', type: 'bytes32' },
    { internalType: 'address', name: 'recipient', type: 'address' },
    { internalType: 'uint256', name: 'startTime', type: 'uint256' },
    { internalType: 'int8[]', name: 'criteriaProof', type: 'int8[]' },
  ],
  method_id: '87201b41',
  name: 'fulfillAvailableAdvancedOrders',
  outputs: [
    { internalType: 'bool[]', name: '', type: 'bool[]' },
    {
      components: [
        {
          components: [
            { internalType: 'enum ItemType', name: 'itemType', type: 'uint8' },
            { internalType: 'address', name: 'token', type: 'address' },
            { internalType: 'uint256', name: 'identifier', type: 'uint256' },
            { internalType: 'uint256', name: 'amount', type: 'uint256' },
            { internalType: 'address payable', name: 'recipient', type: 'address' },
          ],
          internalType: 'struct ReceivedItem',
          name: 'item',
          type: 'tuple',
        },
        { internalType: 'address', name: 'offerer', type: 'address' },
        { internalType: 'bytes32', name: 'conduitKey', type: 'bytes32' },
      ],
      internalType: 'struct Execution[]',
      name: '',
      type: 'tuple[]',
    },
  ],
  stateMutability: 'payable',
  type: 'function',
  payable: true,
  constant: false,
};

test('base view +@mobile +@dark-mode', async({ render }) => {

  const component = await render(
    <ContractMethodForm
      data={ data }
      onSubmit={ onSubmit }
      onReset={ onReset }
      isOpen
      attempt={ 0 }
    />,
  );

  // fill top level fields
  await component.getByPlaceholder('address').last().fill('0x0000');
  await component.getByPlaceholder('uint256').last().fill('42');
  await component.getByRole('button', { name: '×' }).last().click();
  await component.getByPlaceholder('bytes32').last().fill('aa');
  await component.getByLabel('add').last().click();
  await component.getByLabel('add').last().click();
  await component.getByPlaceholder('int8', { exact: true }).first().fill('1');
  await component.getByPlaceholder('int8', { exact: true }).last().fill('3');

  // expand all sections
  await component.getByText('parameters').click();
  await component.getByText('additionalRecipients').click();
  await component.getByText('#1 AdditionalRecipient').click();
  await component.getByLabel('add').first().click();
  await component.getByPlaceholder('uint256').nth(1).fill('42');
  await component.getByPlaceholder('address').nth(1).fill('0xd789a607CEac2f0E14867de4EB15b15C9FFB5859');

  await component.getByText('struct FulfillmentComponent[][]').click();
  await component.getByLabel('add').nth(1).click();
  await component.getByText('#1 FulfillmentComponent[]').click();
  await component.getByLabel('#1 FulfillmentComponent[] (tuple[])').getByText('#1 FulfillmentComponent (tuple)').click();
  await component.getByLabel('add').nth(1).click();

  await component.getByText('ParentArray (int256[2][][3])').click();
  await component.getByText('#1 int256[2][] (int256[2][])').click();
  await component.getByLabel('#1 int256[2][] (int256[2][])').getByText('#1 int256[2] (int256[2])').click();

  // submit form
  await component.getByRole('button', { name: 'Write' }).click();

  await expect(component).toHaveScreenshot();
});
