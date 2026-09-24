import type { AccordionItemProps, AccordionRootProps, GridProps } from '@chakra-ui/react';
import { Box, Spinner, HStack, Grid, GridItem } from '@chakra-ui/react';
import React from 'react';

import type { StepStatus } from './types';

import type { AccordionItemContentProps } from 'toolkit/chakra/accordion';
import { AccordionItem, AccordionItemContent, AccordionItemTrigger, AccordionRoot } from 'toolkit/chakra/accordion';
import { Skeleton } from 'toolkit/chakra/skeleton';
import IconSvg from 'ui/shared/IconSvg';

export const Root = (props: AccordionRootProps) => {
  return (
    <AccordionRoot maxW="800px" display="flex" flexDirection="column" rowGap={ 6 } lazyMount { ...props }/>
  );
};

interface TriggerProps {
  status: StepStatus;
  text: string;
  isFirst: boolean;
  isLast: boolean;
  isLoading?: boolean;
  isDisabled?: boolean;
}

export const Trigger = ({ status, text, isFirst, isLast, isLoading, isDisabled }: TriggerProps) => {
  const content = (() => {
    switch (status) {
      case 'pending': {
        return (
          <HStack gap={ 2 }>
            <Spinner size="md"/>
            <Box color="text.secondary">
              { text }
            </Box>
          </HStack>
        );
      }
      default: {
        const { icon, color } = (() => {
          switch (status) {
            case 'success': {
              return { icon: 'verification-steps/finalized' as const, color: 'green.500' };
            }
            case 'error': {
              return { icon: 'verification-steps/error' as const, color: 'red.600' };
            }
            case 'unfinalized': {
              return { icon: 'verification-steps/unfinalized' as const, color: 'text.secondary' };
            }
          }
        })();
        return (
          <HStack gap={ 2 } color={ color }>
            <IconSvg name={ icon } boxSize={ 5 } isLoading={ isLoading }/>
            <Skeleton loading={ isLoading }>
              { text }
            </Skeleton>
          </HStack>
        );
      }
    }
  })();

  return (
    <AccordionItemTrigger
      position="relative"
      pt={ isFirst ? 0 : 1 }
      pb={ 1 }
      _before={ !isFirst ? {
        position: 'absolute',
        left: '9px',
        bottom: 'calc(100% - 6px)',
        width: '0',
        height: '30px',
        borderColor: 'border.divider',
        borderLeftWidth: '2px',
        content: '""',
      } : undefined }
      _after={ !isLast ? {
        position: 'absolute',
        left: '9px',
        top: 'calc(100% - 6px)',
        width: '0',
        height: '6px',
        borderColor: 'border.divider',
        borderLeftWidth: '2px',
        content: '""',
      } : undefined }
      _open={{
        _after: {
          height: { base: '14px', lg: '6px' },
        },
      }}
      disabled={ isLoading || isDisabled }
      noIndicator={ isLoading || isDisabled }
      cursor={ isDisabled ? 'default' : 'pointer' }
      _disabled={{
        opacity: isDisabled ? 1 : 'control.disabled',
      }}
    >
      { content }
    </AccordionItemTrigger>
  );
};

export const Item = (props: AccordionItemProps) => {
  return <AccordionItem borderBottomWidth="0px" { ...props }/>;
};

interface ContentProps extends AccordionItemContentProps {
  isLast?: boolean;
}

export const ItemContent = ({ isLast, ...rest }: ContentProps) => {
  return (
    <AccordionItemContent
      ml={{ base: 0, lg: '9px' }}
      pl={{ base: 0, lg: '17px' }}
      pt={ 2 }
      pb={ 0 }
      borderLeftWidth={{ base: 0, lg: '2px' }}
      borderColor={ isLast ? 'transparent' : 'border.divider' }
      { ...rest }
    />
  );
};

export const ItemBody = (props: GridProps) => {
  return (
    <Grid
      gridTemplateColumns="112px minmax(0, 1fr)"
      alignItems="flex-start"
      columnGap={ 3 }
      rowGap={ 1 }
      bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
      p="6px"
      pl="18px"
      textStyle="sm"
      borderBottomLeftRadius="base"
      borderBottomRightRadius="base"
      { ...props }
    />
  );
};

interface ItemRowProps {
  label: string;
  children: React.ReactNode;
}

export const ItemRow = ({ label, children }: ItemRowProps) => {
  return (
    <>
      <GridItem color="text.secondary" py="6px">
        { label }
      </GridItem>
      <GridItem>
        { children }
      </GridItem>
    </>
  );
};
