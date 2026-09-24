import React from 'react';

import Pagination from 'ui/shared/pagination/Pagination';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const PaginationShowcase = () => {
  const [ page, setPage ] = React.useState(1);

  const handleNextPageClick = () => {
    setPage(page + 1);
  };

  const handlePrevPageClick = () => {
    setPage(page - 1);
  };

  const props = {
    page,
    onNextPageClick: handleNextPageClick,
    onPrevPageClick: handlePrevPageClick,
    resetPage: () => setPage(1),
    hasNextPage: page < 10,
    hasPages: true,
    isVisible: true,
    canGoBackwards: true,
    isLoading: false,
  };

  return (
    <Container value="pagination">
      <Section>
        <SectionHeader>Examples</SectionHeader>

        <SectionSubHeader>List pagination</SectionSubHeader>
        <SamplesStack>
          <Sample label="Loaded state">
            <Pagination { ...props }/>
          </Sample>
          <Sample label="Initial loading state">
            <Pagination { ...props } hasPages={ false } page={ 1 } isLoading/>
          </Sample>
          <Sample label="Next page loading state">
            <Pagination { ...props } isLoading/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(PaginationShowcase);
