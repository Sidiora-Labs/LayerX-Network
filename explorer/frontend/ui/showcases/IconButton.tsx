import React from 'react';

import { Checkbox } from 'toolkit/chakra/checkbox';
import { IconButton } from 'toolkit/chakra/icon-button';
import { PopoverBody, PopoverContent, PopoverTrigger, PopoverRoot } from 'toolkit/chakra/popover';
import IconSvg from 'ui/shared/IconSvg';

import { Section, Container, SectionHeader, SamplesStack, Sample } from './parts';

const IconButtonShowcase = () => {
  return (
    <Container value="icon-button">
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: icon_secondary">
            <IconButton size="md" variant="icon_secondary">
              <IconSvg name="info"/>
            </IconButton>
            <IconButton size="md" variant="icon_secondary" data-hover>
              <IconSvg name="info"/>
            </IconButton>
            <IconButton size="md" variant="icon_secondary" disabled>
              <IconSvg name="info"/>
            </IconButton>
            <IconButton size="md" variant="icon_secondary" selected>
              <IconSvg name="info"/>
            </IconButton>
            <IconButton size="md" variant="icon_secondary" selected data-hover>
              <IconSvg name="info"/>
            </IconButton>
            <IconButton size="md" variant="icon_secondary" selected disabled>
              <IconSvg name="info"/>
            </IconButton>
          </Sample>

          <Sample label="variant: icon_background">
            <IconButton size="md" variant="icon_background">
              <IconSvg name="heart_outline"/>
            </IconButton>
            <IconButton size="md" variant="icon_background" data-hover>
              <IconSvg name="heart_outline"/>
            </IconButton>
            <IconButton size="md" variant="icon_background" disabled>
              <IconSvg name="heart_outline"/>
            </IconButton>
            <IconButton size="md" variant="icon_background" selected>
              <IconSvg name="heart_filled"/>
            </IconButton>
            <IconButton size="md" variant="icon_background" selected data-hover>
              <IconSvg name="heart_filled"/>
            </IconButton>
            <IconButton size="md" variant="icon_background" selected disabled>
              <IconSvg name="heart_filled"/>
            </IconButton>
          </Sample>

          <Sample label="variant: dropdown">
            <PopoverRoot>
              <PopoverTrigger>
                <IconButton size="md" variant="dropdown">
                  <IconSvg name="filter"/>
                </IconButton>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  Popover content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <IconButton size="md" variant="dropdown" expanded>
                  <IconSvg name="filter"/>
                </IconButton>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  Popover content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <IconButton size="md" variant="dropdown" selected>
                  <IconSvg name="filter"/>
                </IconButton>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody display="flex" flexDirection="column" gap={ 2 }>
                  <Checkbox defaultChecked>First option</Checkbox>
                  <Checkbox>Second option</Checkbox>
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <IconButton size="md" variant="dropdown" disabled>
              <IconSvg name="filter"/>
            </IconButton>
            <IconButton size="md" variant="dropdown" loading>
              <IconSvg name="filter"/>
            </IconButton>
            <IconButton size="md" variant="dropdown" loadingSkeleton>
              <IconSvg name="filter"/>
            </IconButton>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: 2xs">
            <IconButton size="2xs" variant="icon_secondary" outline="1px dashed lightpink">
              <IconSvg name="star_outline"/>
            </IconButton>
          </Sample>
          <Sample label="size: 2xs_alt">
            <IconButton size="2xs_alt" variant="icon_secondary" outline="1px dashed lightpink">
              <IconSvg name="plus"/>
            </IconButton>
          </Sample>
          <Sample label="size: md">
            <IconButton size="md" variant="icon_secondary" outline="1px dashed lightpink">
              <IconSvg name="star_outline"/>
            </IconButton>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(IconButtonShowcase);
