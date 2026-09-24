import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import type { AddressMetadataTagFormatted } from 'types/client/addressMetadata';

import type { AlertProps } from 'toolkit/chakra/alert';
import { Alert } from 'toolkit/chakra/alert';

interface Props {
  tags: Array<AddressMetadataTagFormatted> | undefined;
  isScamToken?: boolean;
  className?: string;
}

const AddressAlerts = ({ tags, isScamToken, className }: Props) => {
  const noteTags = tags?.filter(({ tagType }) => tagType === 'note').filter(({ meta }) => meta?.data);

  if (!noteTags?.length && !isScamToken) {
    return null;
  }

  return (
    <Flex flexDir="column" rowGap={{ base: 1, lg: 2 }} mb={ 3 } className={ className }>
      { isScamToken && (
        <Alert status="error">
          This token has been flagged as a potential scam.
        </Alert>
      ) }
      { noteTags?.map((noteTag) => (
        <Alert
          key={ noteTag.name }
          status={ noteTag.meta?.alertStatus as AlertProps['status'] ?? 'error' }
          bgColor={ noteTag.meta?.alertBgColor }
          color={ noteTag.meta?.alertTextColor }
          whiteSpace="pre-wrap"
          display="inline-block"
          css={{
            '& a': {
              color: 'link.primary',
              _hover: {
                color: 'link.primary.hover',
              },
            },
          }}
        >
          <div dangerouslySetInnerHTML={{ __html: noteTag.meta?.data ?? '' }}/>
        </Alert>
      )) }
    </Flex>
  );
};

export default React.memo(chakra(AddressAlerts));
