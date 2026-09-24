import { Box } from '@chakra-ui/react';
import React from 'react';

import { Alert } from 'toolkit/chakra/alert';
import { Link } from 'toolkit/chakra/link';
import { TableBody, TableColumnHeader, TableHeader, TableRoot, TableRow } from 'toolkit/chakra/table';
import * as SocketNewItemsNotice from 'ui/shared/SocketNewItemsNotice';
import TxPendingAlert from 'ui/tx/TxPendingAlert';

import { Section, Container, SectionHeader, SamplesStack, Sample, SectionSubHeader } from './parts';

const AlertShowcase = () => {
  return (
    <Container value="alert">
      <Section>
        <SectionHeader>Status</SectionHeader>
        <SamplesStack>
          <Sample label="status: info">
            <Alert status="info" title="Info"> Alert content </Alert>
          </Sample>
          <Sample label="status: warning">
            <Alert status="warning" title="Warning"> Alert content </Alert>
          </Sample>
          <Sample label="status: success">
            <Alert status="success" title="Success"> Alert content </Alert>
          </Sample>
          <Sample label="status: error">
            <Alert status="error" title="Error"> Alert content </Alert>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Variant</SectionHeader>
        <SamplesStack>
          <Sample label="variant: subtle">
            <Alert status="info" title="Info" showIcon> Alert content </Alert>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Size</SectionHeader>
        <SamplesStack>
          <Sample label="size: sm">
            <Alert status="info" title="Info" showIcon size="sm" closable> Alert content </Alert>
          </Sample>
          <Sample label="size: md">
            <Alert status="info" title="Info" showIcon size="md"> Alert content </Alert>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Closable</SectionHeader>
        <SamplesStack>
          <Sample label="closable">
            <Alert status="info" title="Info" showIcon closable> Alert content </Alert>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Loading</SectionHeader>
        <SamplesStack>
          <Sample label="loading: true">
            <Alert status="info" title="Info" loading> Alert content </Alert>
          </Sample>
        </SamplesStack>
      </Section>
      <Section>
        <SectionHeader>Examples</SectionHeader>
        <SectionSubHeader>As Link</SectionSubHeader>
        <SamplesStack>
          <Sample>
            <Link href="/" asChild>
              <Alert status="info" title="Info"> Alert content </Alert>
            </Link>
          </Sample>
        </SamplesStack>
        <SectionSubHeader>Inside table (SocketNewItemsNotice)</SectionSubHeader>
        <SamplesStack>
          <Sample label="loading">
            <TableRoot tableLayout="auto">
              <TableHeader>
                <TableRow>
                  <TableColumnHeader w="100px">Block</TableColumnHeader>
                  <TableColumnHeader w="100px">Age</TableColumnHeader>
                  <TableColumnHeader w="100px">Gas used</TableColumnHeader>
                </TableRow>
              </TableHeader>
              <TableBody>
                <SocketNewItemsNotice.Desktop
                  num={ 1234 }
                  type="block"
                  isLoading
                />
              </TableBody>
            </TableRoot>
          </Sample>
          <Sample label="success">
            <TableRoot tableLayout="auto">
              <TableHeader>
                <TableRow>
                  <TableColumnHeader w="100px">Block</TableColumnHeader>
                  <TableColumnHeader w="100px">Age</TableColumnHeader>
                  <TableColumnHeader w="100px">Gas used</TableColumnHeader>
                </TableRow>
              </TableHeader>
              <TableBody>
                <SocketNewItemsNotice.Desktop
                  num={ 1234 }
                  type="block"
                  isLoading={ false }
                />
              </TableBody>
            </TableRoot>
          </Sample>
        </SamplesStack>

        <SectionSubHeader>Multiple lines</SectionSubHeader>
        <SamplesStack>
          <Sample label="multiple lines, with title, inline=false">
            <Alert status="warning" title="Warning" inline={ false } maxWidth="500px" showIcon closable>
              <Box>
                Participated in our recent Blockscout activities? Check your eligibility and claim your NFT Scout badges. More exciting things are coming soon!
              </Box>
            </Alert>
          </Sample>
          <Sample label="multiple lines, no title">
            <Alert status="warning" maxWidth="500px">
              <Box>
                Participated in our recent Blockscout activities? Check your eligibility and claim your NFT Scout badges. More exciting things are coming soon!
              </Box>
            </Alert>
          </Sample>
          <Sample label="with spinner">
            <TxPendingAlert/>
          </Sample>
        </SamplesStack>
      </Section>
    </Container>
  );
};

export default React.memo(AlertShowcase);
