import type { ButtonProps } from '@chakra-ui/react';
import { Icon, useRecipe } from '@chakra-ui/react';
import * as React from 'react';

import CloseIcon from 'icons/close.svg';

import { recipe as closeButtonRecipe } from '../theme/recipes/close-button.recipe';
import { IconButton } from './icon-button';
export interface CloseButtonProps extends Omit<ButtonProps, 'variant' | 'size'> {
  variant?: 'plain';
  size?: 'md';
}

export const CloseButton = React.forwardRef<
  HTMLButtonElement,
  CloseButtonProps
>(function CloseButton(props, ref) {
  const recipe = useRecipe({ recipe: closeButtonRecipe });
  const [ recipeProps, restProps ] = recipe.splitVariantProps(props);
  const styles = recipe(recipeProps);

  return (
    <IconButton aria-label="Close" ref={ ref } css={ styles } { ...restProps }>
      { props.children ?? <Icon boxSize={ 5 }><CloseIcon/></Icon> }
    </IconButton>
  );
});
