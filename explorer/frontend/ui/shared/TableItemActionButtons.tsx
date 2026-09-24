import { HStack } from '@chakra-ui/react';
import React from 'react';

import usePreventFocusAfterModalClosing from 'lib/hooks/usePreventFocusAfterModalClosing';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

type Props = {
  onEditClick: () => void;
  onDeleteClick: () => void;
  isLoading?: boolean;
};

const TableItemActionButtons = ({ onEditClick, onDeleteClick, isLoading }: Props) => {
  const onFocusCapture = usePreventFocusAfterModalClosing();

  return (
    <HStack gap={ 6 } alignSelf="flex-end">
      <Tooltip content="Edit" disableOnMobile>
        <IconButton
          aria-label="edit"
          variant="link"
          size="2xs"
          onClick={ onEditClick }
          onFocusCapture={ onFocusCapture }
          loadingSkeleton={ isLoading }
          borderRadius="none"
        >
          <IconSvg name="edit"/>
        </IconButton>
      </Tooltip>
      <Tooltip content="Delete" disableOnMobile>
        <IconButton
          aria-label="delete"
          variant="link"
          size="2xs"
          onClick={ onDeleteClick }
          onFocusCapture={ onFocusCapture }
          loadingSkeleton={ isLoading }
          borderRadius="none"
        >
          <IconSvg name="delete"/>
        </IconButton>
      </Tooltip>
    </HStack>
  );
};

export default React.memo(TableItemActionButtons);
