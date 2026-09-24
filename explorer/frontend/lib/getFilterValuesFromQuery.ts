import getValuesArrayFromQuery from './getValuesArrayFromQuery';

export default function getFilterValue<FilterType>(filterValues: ReadonlyArray<FilterType>, val: string | Array<string> | undefined) {
  const valArray = getValuesArrayFromQuery(val);

  if (!valArray) {
    return;
  }

  return valArray.filter(el => filterValues.includes(el as unknown as FilterType)) as unknown as Array<FilterType>;
}
