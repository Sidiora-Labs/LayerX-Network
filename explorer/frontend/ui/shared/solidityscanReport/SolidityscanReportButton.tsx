import { Spinner, Box } from '@chakra-ui/react';
import React from 'react';

import usePreventFocusAfterModalClosing from 'lib/hooks/usePreventFocusAfterModalClosing';
import type { ButtonProps } from 'toolkit/chakra/button';
import { Button } from 'toolkit/chakra/button';
import { PopoverTrigger } from 'toolkit/chakra/popover';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

import useScoreLevelAndColor from './useScoreLevelAndColor';

interface Props extends ButtonProps {
  score: number;
  isLoading?: boolean;
  tooltipDisabled?: boolean;
}

const SolidityscanReportButton = ({ score, isLoading, tooltipDisabled, ...rest }: Props) => {
  const { scoreColor } = useScoreLevelAndColor(score);
  const colorLoading = { _light: 'gray.300', _dark: 'gray.600' };
  const onFocusCapture = usePreventFocusAfterModalClosing();

  return (
    <Tooltip content="Security score" disabled={ tooltipDisabled } disableOnMobile closeOnClick>
      <Box>
        <PopoverTrigger>
          <Button
            color={ isLoading ? colorLoading : scoreColor }
            size="sm"
            variant="dropdown"
            aria-label="SolidityScan score"
            fontWeight={ 500 }
            px="6px"
            flexShrink={ 0 }
            columnGap={ 1 }
            disabled={ isLoading }
            _hover={{ color: 'hover' }}
            _expanded={{ color: 'hover' }}
            _disabled={{
              opacity: 1,
              _hover: { color: colorLoading },
            }}
            onFocusCapture={ onFocusCapture }
            { ...rest }
          >
            <IconSvg name={ score < 80 ? 'score/score-not-ok' : 'score/score-ok' } boxSize={ 5 }/>
            { isLoading && <Spinner size="sm"/> }
            { !isLoading && score }
          </Button>
        </PopoverTrigger>
      </Box>
    </Tooltip>
  );
};

export default SolidityscanReportButton;
