import React from 'react';

import type { IconButtonProps } from 'toolkit/chakra/icon-button';
import { IconButton } from 'toolkit/chakra/icon-button';
import IconSvg from 'ui/shared/IconSvg';

interface Props extends IconButtonProps {}

const AddButton = (props: Props) => {
  return (
    <IconButton
      aria-label="Add item"
      variant="icon_secondary"
      size="md"
      { ...props }
    >
      <IconSvg name="plus"/>
    </IconButton>
  );
};

export default React.memo(AddButton);
