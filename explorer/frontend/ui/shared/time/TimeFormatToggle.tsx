import type { StackProps } from '@chakra-ui/react';
import { chakra, HStack } from '@chakra-ui/react';
import React from 'react';

import { useSettingsContext } from 'lib/contexts/settings';
import * as mixpanel from 'lib/mixpanel/index';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

interface Props extends StackProps {}

const TimeFormatToggle = (props: Props) => {
  const settings = useSettingsContext();
  const timeFormat = settings?.timeFormat || 'relative';

  const handleClick = React.useCallback(() => {
    settings?.toggleTimeFormat();
    mixpanel.logEvent(mixpanel.EventTypes.BUTTON_CLICK, { Content: 'Switch time format', Source: 'Table header' });
  }, [ settings ]);

  const text = (() => {
    if (timeFormat === 'relative') {
      return null;
    }

    return <chakra.span color="icon.secondary">{ settings?.isLocalTime ? 'Local' : 'UTC' }</chakra.span>;
  })();

  return (
    <HStack display="inline-flex" gap={ 1 } ml={ 2 } verticalAlign="bottom" { ...props }>
      <Tooltip content="Toggle time format">
        <IconButton
          aria-label="Toggle time format"
          variant="icon_secondary"
          onClick={ handleClick }
          boxSize={ 5 }
          selected={ timeFormat === 'absolute' }
          borderRadius="sm"
          verticalAlign="bottom"
        >
          <IconSvg name="clock-light" boxSize="14px"/>
        </IconButton>
      </Tooltip>
      { text }
    </HStack>
  );
};

export default React.memo(TimeFormatToggle);
