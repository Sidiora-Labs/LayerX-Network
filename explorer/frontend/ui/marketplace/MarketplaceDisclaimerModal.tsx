import { Text } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Button } from 'toolkit/chakra/button';
import { DialogBody, DialogContent, DialogFooter, DialogHeader, DialogRoot } from 'toolkit/chakra/dialog';
import { Link } from 'toolkit/chakra/link';

type Props = {
  isOpen: boolean;
  onClose: () => void;
  appId: string;
};

const MarketplaceDisclaimerModal = ({ isOpen, onClose, appId }: Props) => {

  const isMobile = useIsMobile();

  const handleContinueClick = React.useCallback(() => {
    window.localStorage.setItem('marketplace-disclaimer-shown', 'true');
  }, [ ]);

  return (
    <DialogRoot
      open={ isOpen }
      onOpenChange={ onClose }
      size={ isMobile ? 'full' : 'md' }
    >
      <DialogContent>
        <DialogHeader>
          Disclaimer
        </DialogHeader>

        <DialogBody>
          <Text color={{ _light: 'gray.800', _dark: 'whiteAlpha.800' }}>
            You are now accessing a third-party app. Blockscout does not own, control, maintain, or audit 3rd party apps,{ ' ' }
            and is not liable for any losses associated with these interactions. Please do so at your own risk.
            <br/><br/>
            By clicking continue, you agree that you understand the risks and have read the Disclaimer.
          </Text>
        </DialogBody>

        <DialogFooter
          display="flex"
          flexDirection="row"
          alignItems="center"
        >
          <Link href={ route({ pathname: '/apps/[id]', query: { id: appId } }) } asChild>
            <Button onClick={ handleContinueClick } >
              Continue to app
            </Button>
          </Link>
          <Button
            variant="outline"
            onClick={ onClose }
          >
            Cancel
          </Button>
        </DialogFooter>
      </DialogContent>
    </DialogRoot>
  );
};

export default MarketplaceDisclaimerModal;
