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

    expect(container.querySelectorAll('[data-asset]')).toHaveLength(2);
    expect(screen.getByText('HPX')).toBeTruthy();
    expect(screen.getByText('USDX')).toBeTruthy();
    expect(screen.getByText('3')).toBeTruthy();
    expect(screen.getByText('2.5')).toBeTruthy();
  });

  it('keeps the parts row collapsed until the asset is expanded', () => {
    const { container } = render(<AssetList items={ paxeerXMock.unifiedAccount.balances }/>);

    expect(container.querySelectorAll('[data-parts-of]')).toHaveLength(0);

    fireEvent.click(screen.getByLabelText('Show HPX breakdown'));

    const parts = container.querySelectorAll(`[data-parts-of="${ paxeerXMock.nativeAsset.id }"] [data-part]`);

    expect(parts).toHaveLength(3);
    expect(container.querySelector('[data-part="chain"]')?.textContent).toBe('1');
    expect(container.querySelector('[data-part="custody"]')?.textContent).toBe('1.5');
    expect(container.querySelector('[data-part="kernel"]')?.textContent).toBe('0.5');
  });

  it('collapses the parts row again', () => {
    const { container } = render(<AssetList items={ paxeerXMock.unifiedAccount.balances }/>);

    fireEvent.click(screen.getByLabelText('Show HPX breakdown'));
    fireEvent.click(screen.getByLabelText('Hide HPX breakdown'));

    expect(container.querySelectorAll('[data-parts-of]')).toHaveLength(0);
  });
});
