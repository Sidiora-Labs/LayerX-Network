import React from 'react';

import { Button, ButtonGroupRadio } from 'toolkit/chakra/button';
import { Checkbox } from 'toolkit/chakra/checkbox';
import { Link } from 'toolkit/chakra/link';
import { PopoverContent, PopoverRoot, PopoverTrigger, PopoverBody } from 'toolkit/chakra/popover';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { BACKGROUND_DEFAULT } from 'ui/home/HeroBanner';
import IconSvg from 'ui/shared/IconSvg';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const ButtonShowcase = () => {
  return (
    <Container value="button">
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: 2xs">
            <Button size="2xs">Content</Button>
          </Sample>
          <Sample label="size: xs">
            <Button size="xs">Content</Button>
          </Sample>
          <Sample label="size: sm">
            <Button size="sm">Content</Button>
          </Sample>
          <Sample label="size: md">
            <Button size="md">Content</Button>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Variants</SectionHeader>
        <SamplesStack>

          <Sample label="variant: solid">
            <Button variant="solid">Default</Button>
            <Button variant="solid" data-hover>Hovered</Button>
            <Button variant="solid" disabled>Disabled</Button>
            <Button variant="solid" loading>Loading</Button>
            <Button variant="solid" loadingSkeleton>Loading Skeleton</Button>
          </Sample>

          <Sample label="variant: solid_danger">
            <Button variant="solid_danger">Default</Button>
            <Button variant="solid_danger" data-hover>Hovered</Button>
            <Button variant="solid_danger" disabled>Disabled</Button>
            <Button variant="solid_danger" loading>Loading</Button>
            <Button variant="solid_danger" loadingSkeleton>Loading Skeleton</Button>
          </Sample>

          <Sample label="variant: outline">
            <Button variant="outline">Default</Button>
            <Button variant="outline" data-hover>Hovered</Button>
            <Button variant="outline" disabled>Disabled</Button>
            <Button variant="outline" loading>Loading</Button>
            <Button variant="outline" loadingSkeleton>Loading Skeleton</Button>
          </Sample>

          <Sample label="variant: outline_danger">
            <Button variant="outline_danger">Default</Button>
            <Button variant="outline_danger" data-hover>Hovered</Button>
            <Button variant="outline_danger" disabled>Disabled</Button>
            <Button variant="outline_danger" loading>Loading</Button>
            <Button variant="outline_danger" loadingSkeleton>Loading Skeleton</Button>
          </Sample>

          <Sample label="variant: link">
            <Button variant="link">Default</Button>
            <Button variant="link" data-hover>Hovered</Button>
            <Button variant="link" disabled>Disabled</Button>
            <Button variant="link" loadingSkeleton>Disabled</Button>
          </Sample>

          <Sample label="variant: dropdown">
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="dropdown">Default</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  Popover content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="dropdown" data-hover>Hovered</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  Popover content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="dropdown" expanded>Expended</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  Popover content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="dropdown" selected>Selected</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody display="flex" flexDirection="column" gap={ 2 }>
                  <Checkbox defaultChecked>First option</Checkbox>
                  <Checkbox>Second option</Checkbox>
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <Button variant="dropdown" disabled>Disabled</Button>
            <Button variant="dropdown" loading>Loading</Button>
            <Button variant="dropdown" loadingSkeleton>Loading Skeleton</Button>

            <PopoverRoot>
              <Tooltip content="Tooltip content">
                <div>
                  <PopoverTrigger>
                    <Button variant="dropdown" size="md" px={ 2 }>
                      <IconSvg name="explorer" boxSize={ 5 }/>
                      With tooltip
                    </Button>
                  </PopoverTrigger>
                </div>
              </Tooltip>
              <PopoverContent>
                <PopoverBody>
                  Popover content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
          </Sample>

          <Sample label="variant: header">
            <Button variant="header">Default</Button>
            <Button variant="header" data-hover>Hovered</Button>
            <Button variant="header" loading loadingText="Loading">Loading</Button>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="header" selected>Selected</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  User profile menu content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="header" selected expanded>Selected & expended</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  User profile menu content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="header" selected highlighted>Selected & highlighted</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  User profile menu content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
          </Sample>

          <Sample label="variant: hero" p={ 6 } background={ BACKGROUND_DEFAULT }>
            <Button variant="hero">Default</Button>
            <Button variant="hero" data-hover>Hovered</Button>
            <Button variant="hero" loading loadingText="Loading">Loading</Button>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="hero" selected>Selected</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  User profile menu content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
            <PopoverRoot>
              <PopoverTrigger>
                <Button variant="hero" selected expanded>Selected & expended</Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverBody>
                  User profile menu content
                </PopoverBody>
              </PopoverContent>
            </PopoverRoot>
          </Sample>

          <Sample label="variant: subtle">
            <Button variant="subtle" size="xs">Default: Now+1h</Button>
            <Button variant="subtle" size="xs" data-hover>Hovered: Now+1h</Button>
            <Button variant="subtle" size="xs" disabled>Disabled: Now+1h</Button>
            <Button variant="subtle" size="xs" loadingSkeleton>Loading Skeleton</Button>
          </Sample>

          <Sample label="variant: plain">
            <Button variant="plain">Default</Button>
            <Button variant="plain" data-hover>Hovered</Button>
            <Button variant="plain" disabled>Disabled</Button>
            <Button variant="plain" loadingSkeleton>Loading Skeleton</Button>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Loading</SectionHeader>
        <SamplesStack>
          <Sample label="loading: true, loadingText: 'Loading'">
            <Button loading loadingText="Loading">Content</Button>
            <Button loading loadingText="Loading" size="sm">Content</Button>
            <Button loading loadingText="Loading" size="xs">Content</Button>
            <Button loading loadingText="Loading" size="2xs">Content</Button>
          </Sample>
          <Sample label="loading: true, loadingText: undefined">
            <Button loading>Content</Button>
          </Sample>
          <Sample label="loadingSkeleton: true">
            <Button loadingSkeleton>Content</Button>
            <Button loadingSkeleton variant="outline">Content</Button>
            <Button loadingSkeleton variant="dropdown">Content</Button>
            <Button loadingSkeleton variant="subtle">Content</Button>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Examples</SectionHeader>
        <SectionSubHeader>As Link</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <Link href="/" asChild>
              <Button>I am link</Button>
            </Link>
          </Sample>
        </SamplesStack>

        <SectionSubHeader>Button Group Radio</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <ButtonGroupRadio>
              <Button value="option-1">Option 1</Button>
              <Button value="option-2">Option 2</Button>
              <Button value="option-3">Option 3</Button>
            </ButtonGroupRadio>
            <ButtonGroupRadio loading>
              <Button value="option-1">Option 1</Button>
              <Button value="option-2">Option 2</Button>
              <Button value="option-3">Option 3</Button>
            </ButtonGroupRadio>
          </Sample>
        </SamplesStack>
      </Section>

    </Container>
  );
};

export default React.memo(ButtonShowcase);
