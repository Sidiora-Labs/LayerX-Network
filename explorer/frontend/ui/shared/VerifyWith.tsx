import { Box, chakra, Grid } from '@chakra-ui/react';
import React from 'react';

import { Button } from 'toolkit/chakra/button';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  className?: string;
  links: Array<React.ReactNode>;
  label: string;
  longText: string;
  shortText?: string;
}

const VerifyWith = ({ className, links, label, longText, shortText }: Props) => {
  const popover = useDisclosure();

  return (
    <PopoverRoot open={ popover.open } onOpenChange={ popover.onOpenChange }>
      <Tooltip content={ label } disabled={ popover.open } disableOnMobile closeOnClick>
        <Box className={ className }>
          <PopoverTrigger>
            <Button
              size="sm"
              variant="dropdown"
              aria-label={ label }
              fontWeight={ 500 }
              px={ shortText ? 2 : 1 }
              flexShrink={ 0 }
              columnGap={ 1 }
            >
              <IconSvg name="explorer" boxSize={ 5 }/>
              <chakra.span hideBelow="xl">{ longText }</chakra.span>
              { shortText && <chakra.span hideFrom="xl">{ shortText }</chakra.span> }
            </Button>
          </PopoverTrigger>
        </Box>
      </Tooltip>
      <PopoverContent w="auto">
        <PopoverBody >
          <chakra.span color="text.secondary" textStyle="xs">{ label }</chakra.span>
          <Grid
            alignItems="center"
            templateColumns={ links.length > 1 ? 'auto auto' : '1fr' }
            columnGap={ 4 }
            rowGap={ 2 }
            mt={ 3 }
          >
            { links }
          </Grid>
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default chakra(VerifyWith);
