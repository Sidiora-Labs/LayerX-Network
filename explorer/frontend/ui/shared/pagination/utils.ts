import type { PaginationParams } from './types';

export const emptyPagination: PaginationParams = {
  page: 1,
  onNextPageClick: () => {},
  onPrevPageClick: () => {},
  resetPage: () => {},
  hasPages: false,
  hasNextPage: false,
  canGoBackwards: false,
  isLoading: false,
  isVisible: false,
};
