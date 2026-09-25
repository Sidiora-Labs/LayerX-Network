// @vitest-environment jsdom

import React from 'react';

import * as paxeerXMock from 'mocks/paxeerX/unifiedAccount';
import { describe, expect, it } from 'vitest';
import { fireEvent, screen } from 'vitest/lib';

import AssetList from './AssetList';
import { render } from './testWrapper';

describe('AssetList', () => {
  it('shows an empty message when the account holds nothing', () => {
    render(<AssetList items={ [] }/>);

    expect(screen.getByText('No assets are held by this account.')).toBeTruthy();
  });

  it('renders one row with one total per asset', () => {
    const { container } = render(<AssetList items={ paxeerXMock.unifiedAccount.balances }/>);

    expect(container.querySelectorAll('[data-asset]')).toHaveLength(3);
    expect(container.querySelector(`[data-asset="${ paxeerXMock.nativeAsset.id }"] [data-label="total"]`)?.textContent).toBe('1');
    expect(container.querySelector(`[data-asset="${ paxeerXMock.tokenAsset.id }"] [data-label="total"]`)?.textContent).toBe('2.5');
    expect(container.querySelector(`[data-asset="${ paxeerXMock.custodyAsset.id }"] [data-label="total"]`)?.textContent).toBe('1,200');
    // an asset with a symbol is denominated in it, so the symbol and the denom columns agree
    expect(screen.getAllByText('USDX')).toHaveLength(2);
    expect(screen.getAllByText('HPX')).toHaveLength(2);
  });

  it('keeps the parts row collapsed until the asset is expanded', () => {
    const { container } = render(<AssetList items={ paxeerXMock.unifiedAccount.balances }/>);

    expect(container.querySelectorAll('[data-parts-of]')).toHaveLength(0);

    fireEvent.click(screen.getByLabelText(`Show ${ paxeerXMock.custodyAsset.denom } breakdown`));

    const parts = container.querySelectorAll(`[data-parts-of="${ paxeerXMock.custodyAsset.id }"] [data-part]`);

    expect(parts).toHaveLength(3);
    expect(container.querySelector('[data-part="chain"]')?.textContent).toBe('0');
    expect(container.querySelector('[data-part="custody"]')?.textContent).toBe('500');
    expect(container.querySelector('[data-part="kernel"]')?.textContent).toBe('700');
  });

  it('collapses the parts row again', () => {
    const { container } = render(<AssetList items={ paxeerXMock.unifiedAccount.balances }/>);

    fireEvent.click(screen.getByLabelText('Show HPX breakdown'));
    fireEvent.click(screen.getByLabelText('Hide HPX breakdown'));

    expect(container.querySelectorAll('[data-parts-of]')).toHaveLength(0);
  });
});
