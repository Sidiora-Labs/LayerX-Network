import { describe, it, expect } from 'vitest';

import {
  shouldShowClearButton,
  shouldDisableViewToggle,
  getSearchPlaceholder,
  shouldShowActionBar,
} from './actionBarUtils';

describe('actionBarUtils', () => {
  describe('shouldShowClearButton', () => {
    it('should return true for non-empty search values', () => {
      expect(shouldShowClearButton('test')).toBe(true);
      expect(shouldShowClearButton('a')).toBe(true);
      expect(shouldShowClearButton('cluster-name')).toBe(true);
    });

    it('should return false for empty search values', () => {
      expect(shouldShowClearButton('')).toBe(false);
    });

    it('should return true for whitespace (button should be visible)', () => {
      expect(shouldShowClearButton(' ')).toBe(true);
      expect(shouldShowClearButton('   ')).toBe(true);
    });
  });

  describe('shouldDisableViewToggle', () => {
    it('should return true when loading', () => {
      expect(shouldDisableViewToggle(true)).toBe(true);
    });

    it('should return false when not loading', () => {
      expect(shouldDisableViewToggle(false)).toBe(false);
    });
  });

  describe('getSearchPlaceholder', () => {
    it('should return consistent placeholder text', () => {
      const placeholder = getSearchPlaceholder();
      expect(placeholder).toBe('Search clusters by name or EVM address');
    });

    it('should return same result on multiple calls', () => {
      const first = getSearchPlaceholder();
      const second = getSearchPlaceholder();
      expect(first).toBe(second);
    });
  });

  describe('shouldShowActionBar', () => {
    it('should return true on desktop regardless of pagination', () => {
      expect(shouldShowActionBar(false, true)).toBe(true);
      expect(shouldShowActionBar(true, true)).toBe(true);
    });

    it('should return true on mobile when pagination is visible', () => {
      expect(shouldShowActionBar(true, false)).toBe(true);
    });

    it('should return false on mobile when pagination is not visible', () => {
      expect(shouldShowActionBar(false, false)).toBe(false);
    });
  });
});
