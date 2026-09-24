export default function getNextSortValue<SortField extends string, Sort extends string>(
  sortSequence: Record<SortField, Array<Sort>>, field: SortField,
) {
  return (prevValue: Sort) => {
    const sequence = sortSequence[field];
    return getNextValueFromSequence(sequence, prevValue);
  };
}

export function getNextValueFromSequence<T>(sequence: Array<T>, prevValue: T) {
  const curIndex = sequence.findIndex((val) => val === prevValue);
  const nextIndex = curIndex + 1 > sequence.length - 1 ? 0 : curIndex + 1;
  return sequence[nextIndex];
}

// asc desc undefined
type Order = 'asc' | 'desc' | undefined;
const sequence: Array<Order> = [ 'desc', 'asc', undefined ];
export const getNextOrderValue = (getNextValueFromSequence<Order>).bind(undefined, sequence);
