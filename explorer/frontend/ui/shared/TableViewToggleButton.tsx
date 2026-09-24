import React from 'react';

import { IconButton } from 'toolkit/chakra/icon-button';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  value: boolean;
  onClick: () => void;
  loading?: boolean;
}

const TableViewToggleButton = ({ value, onClick, loading }: Props) => {
  return (
    <IconButton
      size="md"
      variant="dropdown"
      onClick={ onClick }
      selected={ !value }
      loadingSkeleton={ loading }
    >
      <IconSvg name="list_view"/>
    </IconButton>
  );
};

export default React.memo(TableViewToggleButton);
