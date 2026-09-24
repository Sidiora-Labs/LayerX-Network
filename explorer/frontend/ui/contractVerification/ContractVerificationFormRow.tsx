import { chakra, GridItem } from '@chakra-ui/react';
import React from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';

interface Props {
  children: [React.JSX.Element, React.JSX.Element | null] | (React.JSX.Element | null);
  className?: string;
}

const ContractVerificationFormRow = ({ children, className }: Props) => {
  const isMobile = useIsMobile();

  const firstChildren = Array.isArray(children) ? children[0] : children;
  const secondChildren = Array.isArray(children) ? children[1] : null;

  return (
    <>
      <GridItem className={ className } _notFirst={{ mt: { base: 3, lg: 0 } }}>{ firstChildren }</GridItem>
      { isMobile && !secondChildren ? null : <GridItem fontSize="sm" className={ className } color="text.secondary">{ secondChildren }</GridItem> }
    </>
  );
};

export default React.memo(chakra(ContractVerificationFormRow));
