import type { TokenEnhancedData } from 'ui/address/utils/tokenUtils';

export type FormattedData = Record<string, FormattedDataItem>;

export interface FormattedDataItem {
  items: Array<TokenEnhancedData>;
  isOverflow: boolean;
}
