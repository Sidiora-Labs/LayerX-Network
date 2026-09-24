import { Box } from '@chakra-ui/react';
import { useMutation } from '@tanstack/react-query';
import React, { useCallback, useState } from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Button } from 'toolkit/chakra/button';
import { DialogBody, DialogContent, DialogFooter, DialogHeader, DialogRoot } from 'toolkit/chakra/dialog';
import FormSubmitAlert from 'ui/shared/FormSubmitAlert';

type Props = {
  open: boolean;
  onOpenChange: ({ open }: { open: boolean }) => void;
  title: string;
  renderContent: () => React.JSX.Element;
  mutationFn: () => Promise<unknown>;
  onSuccess: () => Promise<void>;
};

const DeleteModal: React.FC<Props> = ({
  open,
  onOpenChange,
  title,
  renderContent,
  mutationFn,
  onSuccess,
}) => {
  const [ isAlertVisible, setAlertVisible ] = useState(false);

  const onModalOpenChange = useCallback(({ open }: { open: boolean }) => {
    !open && setAlertVisible(false);
    onOpenChange({ open });
  }, [ onOpenChange, setAlertVisible ]);

  const { mutate, isPending } = useMutation({
    mutationFn,
    onSuccess: async() => {
      onSuccess();
      onOpenChange({ open: false });
    },
    onError: () => {
      setAlertVisible(true);
    },
  });

  const onDeleteClick = useCallback(() => {
    setAlertVisible(false);
    mutate();
  }, [ setAlertVisible, mutate ]);

  const isMobile = useIsMobile();

  return (
    <DialogRoot open={ open } onOpenChange={ onModalOpenChange } size={ isMobile ? 'full' : 'md' }>
      <DialogContent>
        <DialogHeader>{ title }</DialogHeader>
        <DialogBody>
          { isAlertVisible && <Box mb={ 4 }><FormSubmitAlert/></Box> }
          { renderContent() }
        </DialogBody>
        <DialogFooter>
          <Button
            onClick={ onDeleteClick }
            loading={ isPending }
          >
            Delete
          </Button>
        </DialogFooter>
      </DialogContent>
    </DialogRoot>
  );
};

export default DeleteModal;
