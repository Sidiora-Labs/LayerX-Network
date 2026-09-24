import capitalizeFirstLetter from 'lib/capitalizeFirstLetter';

export const SORT_SEQUENCE: Record<'key0' | 'key1', Array<'desc' | 'asc' | undefined>> = {
  key0: [ 'desc', 'asc', undefined ],
  key1: [ 'desc', 'asc', undefined ],
};

export const getNameTypeText = (name: string, type: string) => {
  return capitalizeFirstLetter(name) + ' (' + type + ')';
};

export const getValueString = (value: string | Array<string>) => {
  if (Array.isArray(value)) {
    return value.join(', ');
  }

  return value.toString();
};
