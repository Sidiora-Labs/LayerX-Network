import React from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';
import useTableViewValue from 'lib/hooks/useTableViewValue';
import ActionBar from 'ui/shared/ActionBar';
import Pagination from 'ui/shared/pagination/Pagination';
import TableViewToggleButton from 'ui/shared/TableViewToggleButton';

import type { Props } from '../txs/TransactionsCrossChainContent';
import TransactionsCrossChainContent from '../txs/TransactionsCrossChainContent';

const AddressCrossChainTxs = ({ pagination, isLoading: isLoadingProp, ...rest }: Props) => {
  const isMobile = useIsMobile();

  const tableViewFlag = useTableViewValue();

  const isTableView = isMobile ? !tableViewFlag.isLoading && tableViewFlag.value : true;
  const isLoading = isLoadingProp || tableViewFlag.isLoading;

  return (
    <>
      { isMobile && (
        <ActionBar>
          <TableViewToggleButton
            value={ tableViewFlag.value }
            onClick={ tableViewFlag.onToggle }
            loading={ isLoading }
          />
          { pagination?.isVisible && <Pagination { ...pagination } ml="auto"/> }
        </ActionBar>
      ) }
      <TransactionsCrossChainContent
        pagination={ pagination }
        isLoading={ isLoading }
        isTableView={ isTableView }
        stickyHeader={ !isMobile && isTableView }
        { ...rest }
      />
    </>
  );
};

export default React.memo(AddressCrossChainTxs);
