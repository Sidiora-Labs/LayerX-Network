import { Icon, RatingGroup, useRatingGroup } from '@chakra-ui/react';
import * as React from 'react';

import StarFilledIcon from 'icons/star_filled.svg';
import StarOutlineIcon from 'icons/star_outline.svg';

export interface RatingProps extends Omit<RatingGroup.RootProviderProps, 'value'> {
  count?: number;
  label?: string | Array<string>;
  defaultValue?: number;
  onValueChange?: ({ value }: { value: number }) => void;
  readOnly?: boolean;
}

export const Rating = React.forwardRef<HTMLDivElement, RatingProps>(
  function Rating(props, ref) {
    const { count = 5, label: labelProp, defaultValue, onValueChange, readOnly, ...rest } = props;
    const store = useRatingGroup({ count, defaultValue, onValueChange, readOnly });

    const highlightedIndex = store.hovering && !readOnly ? store.hoveredValue : store.value;
    const label = Array.isArray(labelProp) ? labelProp[highlightedIndex - 1] : labelProp;

    return (
      <RatingGroup.RootProvider ref={ ref } value={ store } { ...rest }>
        <RatingGroup.HiddenInput/>
        <RatingGroup.Control>
          { Array.from({ length: count }).map((_, index) => {
            const icon = index < highlightedIndex ?
              <Icon boxSize={ 5 }><StarFilledIcon/></Icon> :
              <Icon boxSize={ 5 }><StarOutlineIcon/></Icon>;

            return (
              <RatingGroup.Item key={ index } index={ index + 1 }>
                <RatingGroup.ItemIndicator icon={ icon } cursor={ readOnly ? 'default' : 'pointer' }/>
              </RatingGroup.Item>
            );
          }) }
        </RatingGroup.Control>
        { label && <RatingGroup.Label>{ label }</RatingGroup.Label> }
      </RatingGroup.RootProvider>
    );
  },
);
