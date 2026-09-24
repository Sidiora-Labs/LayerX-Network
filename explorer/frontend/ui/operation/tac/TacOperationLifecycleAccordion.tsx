import React from 'react';

import * as tac from '@blockscout/tac-operation-lifecycle-types';

import { STATUS_LABELS } from 'lib/operations/tac';
import { Root, Item, Trigger } from 'ui/shared/lifecycle/LifecycleAccordion';

import TacOperationLifecycleAccordionItemContent from './TacOperationLifecycleAccordionItemContent';

interface Props {
  data: tac.OperationDetails['status_history'];
  isLoading?: boolean;
  type: tac.OperationType;
}

const TacOperationLifecycleAccordion = ({ data, isLoading, type }: Props) => {
  const isPending = type === tac.OperationType.PENDING && !isLoading;

  return (
    <Root>
      { data.map((item, index) => {
        const isLast = index === data.length - 1 && !isPending;
        return (
          <Item key={ index } value={ item.type }>
            <Trigger
              status={ item.is_success ? 'success' : 'error' }
              text={ STATUS_LABELS[item.type] }
              isFirst={ index === 0 }
              isLast={ isLast }
              isLoading={ isLoading }
            />
            <TacOperationLifecycleAccordionItemContent
              isLast={ isLast }
              data={ item }
            />
          </Item>
        );
      }) }
      { isPending && (
        <Item value="pending">
          <Trigger
            status="pending"
            text="Pending"
            isFirst={ false }
            isLast={ true }
            isLoading={ isLoading }
            isDisabled
          />
        </Item>
      ) }
    </Root>
  );
};

export default React.memo(TacOperationLifecycleAccordion);
