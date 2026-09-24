import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

import { Link } from 'toolkit/chakra/link';

interface Props {
  className?: string;
}

const AdminSupportText = ({ className }: Props) => {
  return (
    <Box className={ className }>
      <span>Need help? Contact admin team at </span>
      <Link href="mailto:help@blockscout.com">help@blockscout.com</Link>
      <span> for assistance!</span>
    </Box>
  );
};

export default chakra(AdminSupportText);
