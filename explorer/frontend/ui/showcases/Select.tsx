import { Box, createListCollection } from '@chakra-ui/react';
import { noop } from 'es-toolkit';
import React from 'react';

import { Checkbox } from 'toolkit/chakra/checkbox';
import type { SelectOption } from 'toolkit/chakra/select';
import { Select, SelectAsync } from 'toolkit/chakra/select';
import PopoverFilterRadio from 'ui/shared/filters/PopoverFilterRadio';
import IconSvg from 'ui/shared/IconSvg';
import Sort from 'ui/shared/sort/Sort';
import TokenTransferFilter from 'ui/shared/TokenTransfer/TokenTransferFilter';
import { SORT_OPTIONS } from 'ui/txs/useTxsSort';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const frameworks = createListCollection<SelectOption>({
  items: [
    { label: 'React.js is the most popular framework', value: 'react', icon: <IconSvg name="navigation/api_keys" boxSize={ 5 } flexShrink={ 0 }/> },
    { label: 'Vue.js is the second most popular framework', value: 'vue' },
    { value: 'angular', label: 'Angular', renderLabel: () => <div>Angular is <Box as="span" color="red" fontWeight="700">not awesome</Box></div> },
    { label: 'Svelte', value: 'svelte' },
  ],
});
const txSortingOptions = createListCollection<SelectOption>({
  items: SORT_OPTIONS,
});

const SelectShowcase = () => {
  const [ hasActiveFilter, setHasActiveFilter ] = React.useState(false);

  const handleFilterChange = React.useCallback((nextValue: string) => {
    setHasActiveFilter(nextValue !== txSortingOptions.items[0].value);
  }, []);

  return (
    <Container value="select">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: outline">
            <Select
              collection={ frameworks }
              placeholder="Select framework"
              size="sm"
              w="200px"
            />
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          { [ 'sm' as const, 'lg' as const ].map((size) => {

            return (
              <Sample label={ `size: ${ size }` } key={ size }>
                <Select
                  collection={ frameworks }
                  placeholder="Select framework"
                  size={ size }
                  w="300px"
                />
                <Select
                  collection={ frameworks }
                  placeholder="Select framework"
                  defaultValue={ [ frameworks.items[0].value ] }
                  size={ size }
                  w="300px"
                />
                <Select
                  collection={ frameworks }
                  placeholder="Select framework"
                  defaultValue={ [ frameworks.items[0].value ] }
                  size={ size }
                  w="300px"
                  readOnly
                />
                <Select
                  collection={ frameworks }
                  placeholder="Select framework"
                  defaultValue={ [ frameworks.items[0].value ] }
                  size={ size }
                  w="300px"
                  disabled
                />
                <Select
                  collection={ frameworks }
                  placeholder="Select framework"
                  size={ size }
                  w="300px"
                  required
                  invalid
                  errorText="Error message"
                />
                <Select
                  collection={ frameworks }
                  placeholder="Select framework"
                  defaultValue={ [ frameworks.items[0].value ] }
                  size={ size }
                  w="300px"
                  required
                  invalid
                  errorText="Error message"
                />
              </Sample>
            );
          }) }
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>With search (async)</SectionHeader>
        <SamplesStack>
          <Sample label="variant: outline">
            <SelectAsync
              placeholder="Select framework"
              size="lg"
              w="300px"
              // eslint-disable-next-line react/jsx-no-bind
              loadOptions={ () => {
                return Promise.resolve(frameworks);
              } }
              extraControls={ <Checkbox mt={ 2 } size="sm">Include nightly builds</Checkbox> }
            />
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Examples</SectionHeader>

        <SectionSubHeader>Sort</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <Sort
              name="transactions_sorting"
              defaultValue={ [ txSortingOptions.items[0].value ] }
              collection={ txSortingOptions }
              w="fit-content"
            />
            <Sort
              name="transactions_sorting"
              defaultValue={ [ txSortingOptions.items[0].value ] }
              collection={ txSortingOptions }
              isLoading
              w="fit-content"
            />
          </Sample>
        </SamplesStack>

        <SectionSubHeader>Token transfers filter</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <TokenTransferFilter defaultTypeFilters={ [ ] } onTypeFilterChange={ noop } withAddressFilter/>
            <TokenTransferFilter defaultTypeFilters={ [ ] } onTypeFilterChange={ noop } appliedFiltersNum={ 2 }/>
            <TokenTransferFilter defaultTypeFilters={ [ ] } onTypeFilterChange={ noop } appliedFiltersNum={ 2 } isLoading/>
          </Sample>
        </SamplesStack>

        <SectionSubHeader>Radio filter</SectionSubHeader>
        <SamplesStack>
          <Sample flexWrap="nowrap">
            <PopoverFilterRadio
              name="transactions_sorting"
              collection={ txSortingOptions }
              hasActiveFilter={ hasActiveFilter }
              onChange={ handleFilterChange }
            />
            <PopoverFilterRadio
              name="transactions_sorting"
              collection={ txSortingOptions }
              hasActiveFilter
              onChange={ noop }
              isLoading
            />
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(SelectShowcase);
