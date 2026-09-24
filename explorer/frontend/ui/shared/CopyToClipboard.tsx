import React from 'react';

import type { IconButtonProps } from 'toolkit/chakra/icon-button';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { useClipboard } from 'toolkit/hooks/useClipboard';
import IconSvg from 'ui/shared/IconSvg';

export interface Props extends Omit<IconButtonProps, 'type' | 'loading'> {
  text: string;
  type?: 'link' | 'text' | 'share';
  isLoading?: boolean;
  // Chakra v3 doesn't support tooltip inside tooltip - https://github.com/chakra-ui/chakra-ui/issues/9939#issuecomment-2817168121
  // so we disable the copy tooltip manually when the button is inside a tooltip
  noTooltip?: boolean;
  tooltipInteractive?: boolean;
}

const CopyToClipboard = (props: Props) => {
  const { text, type = 'text', isLoading, onClick, boxSize = 5, noTooltip, tooltipInteractive, ...rest } = props;

  const { hasCopied, copy, disclosure } = useClipboard(text);

  const handleClick = React.useCallback((event: React.MouseEvent<HTMLButtonElement, MouseEvent>) => {
    event.stopPropagation();
    copy();
    onClick?.(event);
  }, [ onClick, copy ]);

  const iconName = (() => {
    switch (type) {
      case 'link':
        return hasCopied ? 'check' : 'link';
      case 'share':
        return hasCopied ? 'check' : 'share';
      default:
        return hasCopied ? 'copy_check' : 'copy';
    }
  })();

  const button = (
    <IconButton
      aria-label="copy"
      boxSize={ boxSize }
      onClick={ handleClick }
      ml={ 2 }
      borderRadius="sm"
      loadingSkeleton={ isLoading }
      variant="icon_secondary"
      size="2xs"
      { ...rest }
    >
      <IconSvg name={ iconName }/>
    </IconButton>
  );

  if (noTooltip) {
    return button;
  }

  const tooltipContent = (() => {
    if (hasCopied) {
      return 'Copied';
    }

    if (type === 'link') {
      return 'Copy link to clipboard';
    }

    return 'Copy to clipboard';
  })();

  return (
    <Tooltip
      content={ tooltipContent }
      contentProps={{ zIndex: 'tooltip2' }}
      open={ disclosure.open }
      onOpenChange={ disclosure.onOpenChange }
      closeOnPointerDown={ false }
      interactive={ tooltipInteractive }
    >
      { button }
    </Tooltip>
  );
};

export default React.memo(CopyToClipboard);
