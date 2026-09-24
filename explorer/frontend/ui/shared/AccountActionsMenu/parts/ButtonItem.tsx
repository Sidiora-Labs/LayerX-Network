import React from 'react';

import { IconButton } from 'toolkit/chakra/icon-button';
import { Tooltip } from 'toolkit/chakra/tooltip';
import type { IconName } from 'ui/shared/IconSvg';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  className?: string;
  onClick: () => void;
  label: string;
  icon: IconName | React.ReactElement;
  isDisabled?: boolean;
}

const ButtonItem = ({ className, label, onClick, icon, isDisabled }: Props) => {
  return (
    <Tooltip content={ label } disabled={ isDisabled } disableOnMobile>
      <IconButton
        aria-label={ label }
        className={ className }
        onClick={ onClick }
        disabled={ isDisabled }
        variant="icon_background"
        boxSize={ 8 }
      >
        { typeof icon === 'string' ? <IconSvg name={ icon } boxSize={ 6 }/> : icon }
      </IconButton>
    </Tooltip>
  );
};

export default ButtonItem;
