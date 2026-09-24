import * as paxeerXMock from 'mocks/paxeerX/unifiedAccount';
import { describe, expect, it } from 'vitest';

import { activityKindLabel, assetLabel, formatAmount, listIdentities } from './utils';

describe('listIdentities', () => {
  it('keeps the four spellings in a stable order', () => {
    expect(listIdentities(paxeerXMock.unifiedAccount.identities).map((entry) => entry.kind))
      .toEqual([ 'evm', 'pax', 'did', 'kernel_account' ]);
  });

  it('drops the identities that are absent', () => {
    expect(listIdentities(paxeerXMock.unifiedAccountEvmOnly.identities).map((entry) => entry.kind))
      .toEqual([ 'evm' ]);
  });
});

describe('formatAmount', () => {
  it('scales by the asset decimals', () => {
    expect(formatAmount('1500000000000000000', paxeerXMock.nativeAsset)).toBe('1.5');
    expect(formatAmount('2500000', paxeerXMock.pointerAsset)).toBe('2.5');
  });

  it('leaves the amount unscaled when the asset is unknown', () => {
    expect(formatAmount('1234', null)).toBe('1,234');
  });

  it('returns a non-numeric amount verbatim', () => {
    expect(formatAmount('not-a-number', paxeerXMock.nativeAsset)).toBe('not-a-number');
  });
});

describe('assetLabel', () => {
  it('prefers the symbol, then the denom', () => {
    expect(assetLabel(paxeerXMock.nativeAsset)).toBe('HPX');
    expect(assetLabel({ ...paxeerXMock.nativeAsset, symbol: null })).toBe('uhpx');
    expect(assetLabel(null)).toBe('Unknown asset');
  });
});

describe('activityKindLabel', () => {
  it('names the custody and binding events', () => {
    expect(activityKindLabel('custody-deposit')).toBe('Custody deposit');
    expect(activityKindLabel('claim-finalised')).toBe('Claim finalised');
    expect(activityKindLabel('unbound')).toBe('Account unbound');
  });

  it('humanizes a kind it does not know', () => {
    expect(activityKindLabel('module12/event3')).toBe('Module12/event3');
    expect(activityKindLabel('native_transfer')).toBe('Native transfer');
  });
});
