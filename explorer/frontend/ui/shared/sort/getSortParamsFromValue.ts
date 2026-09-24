export default function getSortParamsFromValue<SortValue extends string, SortField extends string, SortOrder extends string>(val?: SortValue) {
  if (!val || val === 'default') {
    return undefined;
  }

  const sortingChunks = val.split('-') as [ SortField, SortOrder ];
  return { sort: sortingChunks[0], order: sortingChunks[1] };
}
