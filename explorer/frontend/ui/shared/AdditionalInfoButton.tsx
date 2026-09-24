import { chakra } from '@chakra-ui/react';
import React from 'react';

import { IconButton } from 'toolkit/chakra/icon-button';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  loading?: boolean;
  className?: string;
}

const AdditionalInfoButton = (props: Props, ref: React.ForwardedRef<HTMLButtonElement>) => {
  const { loading, ...rest } = props;

  return (
    <IconButton
      ref={ ref }
      variant="icon_secondary"
      _open={{
        bgColor: 'selected.control.bg',
        color: 'selected.control.text',
      }}
      borderRadius="base"
      aria-label="Transaction info"
      boxSize={ 6 }
      loadingSkeleton={ loading }
      { ...rest }
    >
      <IconSvg name="info" boxSize={ 5 }/>
    </IconButton>
  );
};

export default chakra(React.forwardRef(AdditionalInfoButton));
