import React from 'react';

import type { GetAvailableBadgesResponse } from '@blockscout/points-types';

import * as streakBadgesMock from 'mocks/rewards/streakBadges';
import type { TestFnArgs } from 'playwright/lib';
import { test, expect, devices } from 'playwright/lib';

import RewardsStreakModal from './RewardsStreakModal';

const onOpenChange = () => {};

const testFn = (streak: number, badges: GetAvailableBadgesResponse['items']) =>
  async({ render, page }: TestFnArgs) => {
    await render(
      <RewardsStreakModal
        open
        onOpenChange={ onOpenChange }
        currentStreak={ streak }
        badges={ badges }
      />,
    );
    await expect(page).toHaveScreenshot();
  };

test('base view +@dark-mode', testFn(10, streakBadgesMock.base.items));
test('filled view +@dark-mode', testFn(10, streakBadgesMock.filled.items));

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });
  test('base view', testFn(10, streakBadgesMock.base.items));
  test('filled view', testFn(10, streakBadgesMock.filled.items));
});
