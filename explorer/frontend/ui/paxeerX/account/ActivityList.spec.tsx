// @vitest-environment jsdom

import React from 'react';

import * as paxeerXMock from 'mocks/paxeerX/unifiedAccount';
import { describe, expect, it } from 'vitest';
import { screen } from 'vitest/lib';

import ActivityList from './ActivityList';
import { render } from './testWrapper';

describe('ActivityList', () => {
  it('shows an empty message when there is no activity', () => {
    render(<ActivityList items={ [] }/>);

    expect(screen.getByText('There is no Paxeer X activity for this account yet.')).toBeTruthy();
  });

  it('renders one row per activity entry', () => {
    const { container } = render(<ActivityList items={ paxeerXMock.unifiedAccount.activity }/>);

    expect(container.querySelectorAll('[data-activity]')).toHaveLength(3);
    expect(screen.getByText('Custody deposit')).toBeTruthy();
    expect(screen.getByText('Claim queued')).toBeTruthy();
  });

  it('humanizes a kind outside the known vocabulary', () => {
    render(<ActivityList items={ paxeerXMock.unifiedAccount.activity }/>);

    expect(screen.getByText('Replay receipt')).toBeTruthy();
  });

  it('puts every row on the status ladder', () => {
    const { container } = render(<ActivityList items={ paxeerXMock.unifiedAccount.activity }/>);

    expect(container.querySelectorAll('[data-rung]')).toHaveLength(3);
    expect(container.querySelector('[data-rung="final"]')).toBeTruthy();
    expect(container.querySelector('[data-rung="sealed"]')).toBeTruthy();
    expect(container.querySelector('[data-rung="instant"]')).toBeTruthy();
  });

  it('scales amounts by the asset decimals and marks the chain and kernel sides', () => {
    render(<ActivityList items={ paxeerXMock.unifiedAccount.activity }/>);

    expect(screen.getByText('1.5 HPX')).toBeTruthy();
    expect(screen.getByText('2.5 USDX')).toBeTruthy();
    expect(screen.getAllByText('Chain')).toHaveLength(2);
    expect(screen.getByText('Kernel')).toBeTruthy();
  });
});
