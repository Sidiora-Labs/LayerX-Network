import { Box } from '@chakra-ui/react';
import React from 'react';

import { TableColumnHeader, TableHeaderSticky, TableRoot, TableRow, TableCell, TableBody, TableColumnHeaderSortable } from 'toolkit/chakra/table';
import getNextSortValue from 'ui/shared/sort/getNextSortValue';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const ITEMS = [
  { id: 1, name: 'Laptop', category: 'Electronics', price: 999.99 },
  { id: 2, name: 'Coffee Maker', category: 'Home Appliances', price: 49.99 },
  { id: 3, name: 'Desk Chair', category: 'Furniture', price: 150.0 },
  { id: 4, name: 'Smartphone', category: 'Electronics', price: 799.99 },
  { id: 5, name: 'Headphones', category: 'Accessories', price: 199.99 },
];

type Item = typeof ITEMS[number];

type SortField = 'category' | 'price';
type SortValue = 'category-asc' | 'category-desc' | 'price-asc' | 'price-desc' | 'default';

const SORT_SEQUENCE: Record<SortField, Array<SortValue>> = {
  category: [ 'category-desc', 'category-asc', 'default' ],
  price: [ 'price-desc', 'price-asc', 'default' ],
};

const TableShowcase = () => {
  const [ sort, setSort ] = React.useState<SortValue>('default');

  const handleSortToggle = React.useCallback((sortField: string) => {
    const value = getNextSortValue<SortField, SortValue>(SORT_SEQUENCE, sortField as SortField)(sort);
    setSort(value);
  }, [ sort, setSort ]);

  const sortFn = (a: Item, b: Item) => {
    if (sort === 'category-asc') {
      return a.category.localeCompare(b.category);
    }
    if (sort === 'category-desc') {
      return b.category.localeCompare(a.category);
    }
    if (sort === 'price-asc') {
      return a.price - b.price;
    }
    if (sort === 'price-desc') {
      return b.price - a.price;
    }
    return 0;
  };

  return (
    <Container value="table">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack >
          <Sample label="variant: line">
            <TableRoot>
              <TableHeaderSticky>
                <TableRow>
                  <TableColumnHeader>Product</TableColumnHeader>
                  <TableColumnHeaderSortable
                    sortField="category"
                    sortValue={ sort }
                    onSortToggle={ handleSortToggle }
                  >
                    Category
                  </TableColumnHeaderSortable>
                  <TableColumnHeaderSortable
                    sortField="price"
                    sortValue={ sort }
                    onSortToggle={ handleSortToggle }
                    isNumeric
                  >
                    Price
                  </TableColumnHeaderSortable>
                </TableRow>
              </TableHeaderSticky>
              <TableBody>
                { ITEMS.slice().sort(sortFn).map((item) => (
                  <TableRow key={ item.id }>
                    <TableCell>{ item.name }</TableCell>
                    <TableCell>{ item.category }</TableCell>
                    <TableCell isNumeric>{ item.price }</TableCell>
                  </TableRow>
                )) }
              </TableBody>
            </TableRoot>
            <Box h="1000px"/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(TableShowcase);
