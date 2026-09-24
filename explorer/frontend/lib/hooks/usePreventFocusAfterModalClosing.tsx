import React from 'react';

// prevent set focus on button when closing modal
export default function usePreventFocusAfterModalClosing() {
  return React.useCallback((e: React.SyntheticEvent) => e.stopPropagation(), []);
}
