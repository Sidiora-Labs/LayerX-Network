import React from 'react';

import type { MethodType } from './types';

import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import DataFetchAlert from 'ui/shared/DataFetchAlert';

interface Props {
  isLoading?: boolean;
  isError?: boolean;
  isEmpty?: boolean;
  type: MethodType;
  children: React.JSX.Element;
}

const ContractMethodsContainer = ({ isLoading, isError, isEmpty, type, children }: Props) => {

  if (isLoading) {
    return <ContentLoader w="fit-content"/>;
  }

  if (isError) {
    return <DataFetchAlert/>;
  }

  if (isEmpty) {
    const typeText = type === 'all' ? '' : type;
    return <span>No public { typeText } functions were found for this contract.</span>;
  }

  return children;
};

export default React.memo(ContractMethodsContainer);
