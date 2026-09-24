export function shouldShowClearButton(searchValue: string): boolean {
  return searchValue.length > 0;
}

export function shouldDisableViewToggle(isLoading: boolean): boolean {
  return isLoading;
}

export function getSearchPlaceholder(): string {
  return 'Search clusters by name or EVM address';
}

export function shouldShowActionBar(paginationVisible: boolean, isDesktop: boolean): boolean {
  return isDesktop || paginationVisible;
}
