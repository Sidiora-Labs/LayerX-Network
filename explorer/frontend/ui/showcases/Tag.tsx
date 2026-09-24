import React from 'react';

import * as addressMetadataMock from 'mocks/metadata/address';
import { Tag } from 'toolkit/chakra/tag';
import EntityTag from 'ui/shared/EntityTags/EntityTag';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const TagShowcase = () => {
  return (
    <Container value="tag">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: subtle">
            <Tag>My tag</Tag>
          </Sample>
          <Sample label="variant: clickable">
            <Tag variant="clickable">My tag</Tag>
          </Sample>
          <Sample label="variant: filter">
            <Tag variant="filter">My tag</Tag>
          </Sample>
          <Sample label="variant: select">
            <Tag variant="select">Default</Tag>
            <Tag variant="select" selected>Selected</Tag>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: md">
            <Tag size="md">My tag</Tag>
          </Sample>
          <Sample label="size: lg">
            <Tag size="lg">My tag</Tag>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Truncated</SectionHeader>
        <SamplesStack>
          <Sample label="truncated: true">
            <Tag maxW="150px" truncated>Very very very very very very very long text</Tag>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Closable</SectionHeader>
        <SamplesStack>
          <Sample label="closable: true">
            <Tag closable>My tag</Tag>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Loading</SectionHeader>
        <SamplesStack>
          <Sample label="loading: true">
            <Tag loading>My tag</Tag>
            <Tag maxW="150px" truncated loading>Very very very very very very very long text</Tag>
          </Sample>
          <Sample label="loading: true, variant: select">
            <Tag variant="select" loading>Default</Tag>
            <Tag variant="select" selected loading>Selected</Tag>
          </Sample>
          <Sample label="loading: false">
            <Tag>My tag</Tag>
            <Tag maxW="150px" truncated>Very very very very very very very long text</Tag>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Examples</SectionHeader>
        <SectionSubHeader>Public tags</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <EntityTag data={ addressMetadataMock.nameTag }/>
            <EntityTag data={ addressMetadataMock.customNameTag }/>
            <EntityTag data={ addressMetadataMock.warpcastTag }/>
            <EntityTag data={ addressMetadataMock.genericTag }/>
            <EntityTag data={ addressMetadataMock.protocolTag }/>
            <EntityTag data={ addressMetadataMock.infoTagWithLink } maxW="150px"/>
            <EntityTag data={ addressMetadataMock.tagWithTooltip }/>
            <EntityTag data={ addressMetadataMock.nameTag } isLoading/>
          </Sample>
        </SamplesStack>

        <SectionSubHeader>Filter tags</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <Tag variant="filter" label="Type">All</Tag>
            <Tag variant="filter" label="Address" truncated maxW="150px" closable>0x1234567890123456789012345678901234567890</Tag>
            <Tag variant="filter" label="Type" loading>All</Tag>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(TagShowcase);
