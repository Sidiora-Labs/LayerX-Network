import { Box } from '@chakra-ui/react';
import React from 'react';

import { TabsContent, TabsList, TabsRoot, TabsTrigger } from 'toolkit/chakra/tabs';
import AdaptiveTabs from 'toolkit/components/AdaptiveTabs/AdaptiveTabs';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const TabsShowcase = () => {
  const tabs = [
    { id: 'tab1', title: 'Swaps', component: <div>Swaps content</div>, count: 10 },
    { id: 'tab2', title: 'Bridges', component: <div>Bridges content</div>, count: 0 },
    { id: 'tab3', title: 'Liquidity staking', component: <div>Liquidity staking content</div>, count: 300 },
    { id: 'tab4', title: 'Lending', component: <div>Lending content</div>, count: 400 },
    { id: 'tab5', title: 'Yield farming', component: <div>Yield farming content</div> },
  ];

  return (
    <Container value="tabs">
      <Section>
        <SectionHeader>Variants</SectionHeader>
        <SamplesStack>
          <Sample label="variant: solid">
            <TabsRoot defaultValue="tab1" variant="solid">
              <TabsList>
                <TabsTrigger value="tab1">First tab</TabsTrigger>
                <TabsTrigger value="tab2">Second tab</TabsTrigger>
              </TabsList>
              <TabsContent value="tab1">First tab content</TabsContent>
              <TabsContent value="tab2">Second tab content</TabsContent>
            </TabsRoot>
          </Sample>
          <Sample label="variant: secondary">
            <TabsRoot defaultValue="tab1" variant="secondary" size="sm">
              <TabsList>
                <TabsTrigger value="tab1">First tab</TabsTrigger>
                <TabsTrigger value="tab2">Second tab</TabsTrigger>
              </TabsList>
              <TabsContent value="tab1">First tab content</TabsContent>
              <TabsContent value="tab2">Second tab content</TabsContent>
            </TabsRoot>
          </Sample>
          <Sample label="variant: segmented">
            <TabsRoot defaultValue="tab1" variant="segmented" size="sm">
              <TabsList>
                <TabsTrigger value="tab1">First tab</TabsTrigger>
                <TabsTrigger value="tab2">Second tab</TabsTrigger>
                <TabsTrigger value="tab3">Third tab</TabsTrigger>
                <TabsTrigger value="tab4">Fourth tab</TabsTrigger>
              </TabsList>
              <TabsContent value="tab1">First tab content</TabsContent>
              <TabsContent value="tab2">Second tab content</TabsContent>
              <TabsContent value="tab3">Third tab content</TabsContent>
              <TabsContent value="tab4">Fourth tab content</TabsContent>
            </TabsRoot>
          </Sample>
        </SamplesStack>
      </Section>

      <Section>
        <SectionHeader>Examples</SectionHeader>

        <SectionSubHeader>Adaptive tabs</SectionSubHeader>
        <SamplesStack>
          <Sample gridColumn="1 / 3" w="100%" maxW={{ base: '100vw', lg: '700px' }}>
            <AdaptiveTabs
              w="100%"
              tabs={ tabs }
              defaultValue={ tabs[0].id }
              outline="1px dashed lightpink"
              leftSlot={ <Box display={{ base: 'none', lg: 'block' }}>Left element</Box> }
              leftSlotProps={{ pr: { base: 0, lg: 4 }, color: 'text.secondary' }}
              rightSlot={ <Box display={{ base: 'none', lg: 'flex' }} justifyContent="space-between"><span>Right element</span><span>🙈</span></Box> }
              rightSlotProps={{ pl: { base: 0, lg: 4 }, color: 'text.secondary', widthAllocation: 'available' }}
            />
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(TabsShowcase);
