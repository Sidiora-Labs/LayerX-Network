import type { StackProps } from '@chakra-ui/react';
import { HStack } from '@chakra-ui/react';
import React from 'react';

import type { TagProps } from 'toolkit/chakra/tag';
import { Tag } from 'toolkit/chakra/tag';

type Props<T extends string> = {
  items: Array<{ id: T; title: string }>;
  tagSize?: TagProps['size'];
  loading?: boolean;
  disabled?: boolean;
} & (
  {
    value?: T;
    onChange: (value: T) => void;
    isMulti?: false;
  } | {
    value: Array<T>;
    onChange: (value: Array<T>) => void;
    isMulti: true;
  }
) & Omit<StackProps, 'onChange'>;

const TagGroupSelect = <T extends string>({ items, value, isMulti, onChange, tagSize, loading, disabled, ...rest }: Props<T>) => {
  const onItemClick = React.useCallback((event: React.SyntheticEvent) => {
    const itemValue = (event.currentTarget as HTMLDivElement).getAttribute('data-id') as T;
    if (isMulti) {
      let newValue;
      if (value.includes(itemValue)) {
        newValue = value.filter(i => i !== itemValue);
      } else {
        newValue = [ ...value, itemValue ];
      }
      onChange(newValue);
    } else {
      onChange(itemValue);
    }
  }, [ isMulti, onChange, value ]);

  return (
    <HStack { ...rest }>
      { items.map(item => {
        const isSelected = isMulti ? value.includes(item.id) : value === item.id;
        return (
          <Tag
            variant="select"
            key={ item.id }
            data-id={ item.id }
            selected={ isSelected }
            fontWeight={ 500 }
            onClick={ disabled ? undefined : onItemClick }
            size={ tagSize }
            display="inline-flex"
            justifyContent="center"
            loading={ loading }
            disabled={ disabled }
          >
            { item.title }
          </Tag>
        );
      }) }
    </HStack>
  );
};

export default TagGroupSelect;
