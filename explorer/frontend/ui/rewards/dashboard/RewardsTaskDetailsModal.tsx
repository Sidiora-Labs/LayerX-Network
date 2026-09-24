import { Text } from '@chakra-ui/react';
import React from 'react';

import { DialogBody, DialogContent, DialogRoot, DialogHeader } from 'toolkit/chakra/dialog';

type Props = {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
};

const RewardsTaskDetailsModal = ({ isOpen, onClose, title, children }: Props) => {
  const handleOpenChange = React.useCallback(({ open }: { open: boolean }) => {
    if (!open) {
      onClose();
    }
  }, [ onClose ]);

  return (
    <DialogRoot
      open={ isOpen }
      onOpenChange={ handleOpenChange }
      size={{ lgDown: 'full', lg: 'sm' }}
    >
      <DialogContent>
        <DialogHeader>
          { title }
        </DialogHeader>
        <DialogBody>
          <Text>{ children }</Text>
          <Text textStyle="sm" color="text.secondary" mt={ 3 }>
            Note: Merits are only earned on supported networks where the program is active.
          </Text>
        </DialogBody>
      </DialogContent>
    </DialogRoot>
  );
};

export default RewardsTaskDetailsModal;
