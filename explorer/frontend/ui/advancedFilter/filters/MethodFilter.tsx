import { Flex, Spinner, chakra } from '@chakra-ui/react';
import { isEqual, differenceBy } from 'es-toolkit';
import React from 'react';

import type { AdvancedFilterMethodInfo, AdvancedFilterParams } from 'types/api/advancedFilter';

import useApiQuery from 'lib/api/useApiQuery';
import useDebounce from 'lib/hooks/useDebounce';
import { Badge } from 'toolkit/chakra/badge';
import { Checkbox, CheckboxGroup } from 'toolkit/chakra/checkbox';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';

const RESET_VALUE = 'all';

const FILTER_PARAM = 'methods';
const NAMES_PARAM = 'methods_names';

type Props = {
  value?: Array<AdvancedFilterMethodInfo>;
  handleFilterChange: (filed: keyof AdvancedFilterParams, val: Array<string>) => void;
};

const MethodFilter = ({ value = [], handleFilterChange }: Props) => {
  const [ currentValue, setCurrentValue ] = React.useState<Array<AdvancedFilterMethodInfo>>([ ...value ]);
  const [ searchTerm, setSearchTerm ] = React.useState<string>('');
  const debouncedSearchTerm = useDebounce(searchTerm, 300);
  const [ methodsList, setMethodsList ] = React.useState<Array<AdvancedFilterMethodInfo>>([]);

  const onSearchChange = React.useCallback((value: string) => {
    setSearchTerm(value);
  }, []);

  const methodsQuery = useApiQuery('general:advanced_filter_methods', {
    queryParams: { q: debouncedSearchTerm },
    queryOptions: { refetchOnMount: false },
  });

  React.useEffect(() => {
    if (!methodsList.length && methodsQuery.data) {
      setMethodsList([ ...value, ...differenceBy(methodsQuery.data, value, i => i.method_id) ]);
    }
  }, [ methodsQuery.data, value, methodsList ]);

  const handleChange: React.FormEventHandler<HTMLLabelElement> = React.useCallback((event) => {
    const checked = (event.target as HTMLInputElement).checked;
    const id = event.currentTarget.getAttribute('data-id');

    if (id === RESET_VALUE) {
      setCurrentValue([]);
      setMethodsList(methodsQuery.data || []);
    } else {
      const methodInfo = methodsQuery.data?.find(m => m.method_id === id);
      if (methodInfo) {
        setCurrentValue(prev => {
          return checked ? [ ...prev, methodInfo ] : prev.filter(i => i.method_id !== id);
        });
        searchTerm && checked &&
        setMethodsList(prev => [ methodInfo, ...(prev.filter(m => m.method_id !== id) || []) ]);
      }
    }
  }, [ methodsQuery.data, searchTerm ]);

  const onReset = React.useCallback(() => setCurrentValue([]), []);

  const onFilter = React.useCallback(() => {
    handleFilterChange(FILTER_PARAM, currentValue.map(item => item.method_id));
    handleFilterChange(NAMES_PARAM, currentValue.map(item => item.name || ''));
  }, [ handleFilterChange, currentValue ]);

  return (
    <TableColumnFilter
      title="Method"
      isFilled={ Boolean(currentValue.length) }
      isTouched={ !isEqual(currentValue.map(i => JSON.stringify(i)).sort(), value.map(i => JSON.stringify(i)).sort()) }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <FilterInput
        size="sm"
        onChange={ onSearchChange }
        placeholder="Find by function name/ method ID"
        mb={ 3 }
      />
      { methodsQuery.isLoading && <Spinner/> }
      { methodsQuery.isError && <span>Something went wrong. Please try again.</span> }
      { Boolean(searchTerm) && methodsQuery.data?.length === 0 && <span>No results found.</span> }
      { methodsQuery.data && (
        // added negative margin because of checkbox focus styles & overflow hidden
        <Flex display="flex" flexDir="column" rowGap={ 3 } maxH="250px" overflowY="scroll">
          <CheckboxGroup
            value={ currentValue.length ? currentValue.map(i => i.method_id) : [ ] }
            orientation="vertical"
          >
            { (searchTerm ? methodsQuery.data : (methodsList || [])).map(method => (
              <Checkbox
                key={ method.method_id }
                value={ method.method_id }
                data-id={ method.method_id }
                onChange={ handleChange }
              >
                <Flex justifyContent="space-between" alignItems="center" id={ method.method_id }>
                  <chakra.span overflow="hidden" whiteSpace="nowrap" textOverflow="ellipsis">{ method.name || method.method_id }</chakra.span>
                  <Badge colorPalette="gray" truncated ml="auto">
                    { method.method_id }
                  </Badge>
                </Flex>
              </Checkbox>
            )) }
          </CheckboxGroup>
        </Flex>
      ) }
    </TableColumnFilter>
  );
};

export default MethodFilter;
