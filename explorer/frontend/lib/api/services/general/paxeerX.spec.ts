import * as capabilitiesMock from 'mocks/paxeerX/capabilities';
import * as unifiedAccountMock from 'mocks/paxeerX/unifiedAccount';
import { PAXEER_X_ANCHORS_ITEM, PAXEER_X_RECEIPTS_ITEM, PAXEER_X_TX_STATUS } from 'stubs/paxeerXLists';
import { describe, expect, it } from 'vitest';

import type { GeneralApiPaxeerXResourceName, GeneralApiPaxeerXResourcePayload } from './paxeerX';
import { GENERAL_API_PAXEER_X_RESOURCES } from './paxeerX';
import type { GeneralApiPaxeerXListsResourceName, GeneralApiPaxeerXListsResourcePayload } from './paxeerXLists';
import { GENERAL_API_PAXEER_X_LISTS_RESOURCES } from './paxeerXLists';

describe('the Paxeer X resource definitions', () => {
  it('carries the unified account and capability paths the backend serves', () => {
    expect(GENERAL_API_PAXEER_X_RESOURCES.paxeer_x_unified_account.path).toBe('/api/v2/addresses/:hash/unified');
    expect(GENERAL_API_PAXEER_X_RESOURCES.paxeer_x_unified_account.pathParams).toEqual([ 'hash' ]);
    expect(GENERAL_API_PAXEER_X_RESOURCES.paxeer_x_capabilities.path).toBe('/api/v2/paxeer-x/capabilities');
  });

  it('takes the transaction hash as a path parameter and paginates the two list paths', () => {
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_tx_status.path).toBe('/api/v2/transactions/:hash/status');
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_tx_status.pathParams).toEqual([ 'hash' ]);
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_anchors.path).toBe('/api/v2/paxeer-x/anchors');
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_anchors.paginated).toBe(true);
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_receipts.path).toBe('/api/v2/paxeer-x/receipts');
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_receipts.paginated).toBe(true);
  });

  it('reads a single receipt by the id in its path', () => {
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_receipt.path).toBe('/api/v2/paxeer-x/receipts/:id');
    expect(GENERAL_API_PAXEER_X_LISTS_RESOURCES.paxeer_x_receipt.pathParams).toEqual([ 'id' ]);
  });
});

describe('the Paxeer X payload map', () => {
  it('answers each resource name with the payload of its own path', () => {
    const unifiedAccount: GeneralApiPaxeerXResourcePayload<'general:paxeer_x_unified_account'> = unifiedAccountMock.unifiedAccount;
    const capabilities: GeneralApiPaxeerXResourcePayload<'general:paxeer_x_capabilities'> = capabilitiesMock.addrOnly;
    const txStatus: GeneralApiPaxeerXListsResourcePayload<'general:paxeer_x_tx_status'> = PAXEER_X_TX_STATUS;
    const anchors: GeneralApiPaxeerXListsResourcePayload<'general:paxeer_x_anchors'> = {
      items: [ PAXEER_X_ANCHORS_ITEM ],
      next_page_params: null,
    };
    const receipts: GeneralApiPaxeerXListsResourcePayload<'general:paxeer_x_receipts'> = {
      items: [ PAXEER_X_RECEIPTS_ITEM ],
      next_page_params: null,
    };
    const receipt: GeneralApiPaxeerXListsResourcePayload<'general:paxeer_x_receipt'> = {
      ...PAXEER_X_RECEIPTS_ITEM,
      verification_status: 'checkpoint_finalised',
      payload_hash: '0x3ed9d81e7c1001bdda1caa1dc62c0acbbe3d2c671cdc20dc1e65efdaa4186967',
      transaction_hash: '0x8f9e7d6c5b4a39281706f5e4d3c2b1a0998877665544332211ffeeddccbbaa99',
      timestamp: '2023-05-22T18:00:36.000000Z',
    };

    expect(unifiedAccount.identities.evm).toBe(unifiedAccountMock.evmAddress);
    expect(unifiedAccount.balances).toHaveLength(3);
    expect(capabilities.addr).toBe(true);
    expect(capabilities.custody).toBe(false);
    expect(txStatus.rung).toBe(PAXEER_X_TX_STATUS.rung);
    expect(anchors.items[0].checkpoint_id).toBe(PAXEER_X_ANCHORS_ITEM.checkpoint_id);
    expect(receipts.items[0].status).toBe(PAXEER_X_RECEIPTS_ITEM.status);
    expect(receipt.id).toBe(PAXEER_X_RECEIPTS_ITEM.id);
    expect(receipt.verification_status).toBe('checkpoint_finalised');
  });

  it('keeps one resource name per definition in both maps', () => {
    const names: Array<GeneralApiPaxeerXResourceName | GeneralApiPaxeerXListsResourceName> = [
      'general:paxeer_x_unified_account',
      'general:paxeer_x_capabilities',
      'general:paxeer_x_tx_status',
      'general:paxeer_x_anchors',
      'general:paxeer_x_receipts',
      'general:paxeer_x_receipt',
    ];

    const defined = [
      ...Object.keys(GENERAL_API_PAXEER_X_RESOURCES),
      ...Object.keys(GENERAL_API_PAXEER_X_LISTS_RESOURCES),
    ].map((name) => `general:${ name }`);

    expect(names).toEqual(defined);
  });
});
