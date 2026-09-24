import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

import type { AlertProps } from 'toolkit/chakra/alert';
import { Alert } from 'toolkit/chakra/alert';

type Props = {
  html: string;
  status: AlertProps['status'];
  showIcon?: boolean;
  className?: string;
};

const AlertWithExternalHtml = ({ html, status, showIcon, className }: Props) => {
  return (
    <Alert status={ status } showIcon={ showIcon } className={ className }>
      <Box
        dangerouslySetInnerHTML={{ __html: html }}
        css={{
          '& a': {
            color: 'link.primary',
            _hover: {
              color: 'link.primary.hover',
            },
          },
        }}

      />
    </Alert>
  );
};

export default React.memo(chakra(AlertWithExternalHtml));
