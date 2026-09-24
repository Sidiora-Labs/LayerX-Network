import { chakra, Text } from '@chakra-ui/react';
import React from 'react';

import { getFeaturePayload } from 'configs/app/features/types';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import usePreventFocusAfterModalClosing from 'lib/hooks/usePreventFocusAfterModalClosing';
import type { ButtonProps } from 'toolkit/chakra/button';
import { Button } from 'toolkit/chakra/button';
import { PopoverTrigger } from 'toolkit/chakra/popover';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

interface Props extends ButtonProps {
  rating?: number;
  count?: number;
  fullView?: boolean;
  canRate: boolean;
};

const getTooltipText = (canRate: boolean) => {
  if (!canRate) {
    return <>Please log in to Blockscout to rate this { (getFeaturePayload(config.features.marketplace)?.titles.entity_name ?? '').toLowerCase() }.</>;
  }
  return <>Ratings come from verified users.<br/>Click here to rate!</>;
};

const TriggerButton = (
  { rating, count, fullView, canRate, onClick, ...rest }: Props,
  ref: React.ForwardedRef<HTMLButtonElement>,
) => {
  const onFocusCapture = usePreventFocusAfterModalClosing();
  const isMobile = useIsMobile();

  return (
    <Tooltip
      content={ getTooltipText(canRate) }
      closeOnClick={ Boolean(canRate) || isMobile }
      disableOnMobile={ canRate }
    >
      <div>
        <PopoverTrigger>
          <Button
            ref={ ref }
            size="xs"
            variant="link"
            p={ 0 }
            fontSize={ fullView ? 'md' : 'sm' }
            fontWeight={ fullView ? '400' : '500' }
            lineHeight="21px"
            ml={ fullView ? 3 : 0 }
            onFocusCapture={ onFocusCapture }
            cursor={ canRate ? 'pointer' : 'default' }
            { ...rest }
          >
            { !fullView && (
              <IconSvg
                name={ rating ? 'star_filled' : 'star_outline' }
                color={ rating ? 'yellow.400' : 'icon.secondary' }
                boxSize={ 5 }
                mr={ 1 }
              />
            ) }
            { (rating && !fullView) ? (
              <chakra.span color="text.primary" transition="inherit" display="inline-flex">
                { rating }
                <Text color="text.secondary" ml={ 1 }>({ count })</Text>
              </chakra.span>
            ) : (
              'Rate it!'
            ) }
          </Button>
        </PopoverTrigger>
      </div>
    </Tooltip>
  );
};

export default React.forwardRef(TriggerButton);
