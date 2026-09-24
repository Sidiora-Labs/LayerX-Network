export interface PaginationParams {
  page: number;
  onNextPageClick: () => void;
  onPrevPageClick: () => void;
  resetPage: () => void;
  hasPages: boolean;
  hasNextPage: boolean;
  canGoBackwards: boolean;
  isLoading: boolean;
  isVisible: boolean;
}
