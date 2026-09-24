import { Box } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { currencyUnits } from 'lib/units';
import { AccordionRoot } from 'toolkit/chakra/accordion';
import { Heading } from 'toolkit/chakra/heading';

import GasTrackerFaqItem from './GasTrackerFaqItem';

const FAQ_ITEMS = [
  {
    question: 'What does gas refer to on the blockchain?',
    answer: 'Gas is the amount of native tokens required to perform a transaction on the blockchain.',
  },
  {
    question: `How can I check ${ config.chain.name } gas fees?`,
    // eslint-disable-next-line max-len
    answer: `You can easily check live ${ config.chain.name } gas fees on Blockscout by visiting our gas tracker. It displays current gas fees in ${ currencyUnits.gwei } for all ${ config.chain.name } transactions.`,
  },
  {
    question: `What is the average gas fee for ${ config.chain.name } transactions?`,
    // eslint-disable-next-line max-len
    answer: `The average gas fee for ${ config.chain.name } transactions depends on network congestion and transaction complexity. Blockscout provides real-time gas fee estimations to help users make informed decisions.`,
  },
  {
    question: 'How does Blockscout calculate gas fees?',
    answer: 'Blockscout calculates gas fees based on the average price of gas fees spent for the last 200 blocks.',
  },
];

const GasTrackerFaq = () => {
  return (
    <Box mt={ 12 }>
      <Heading level="2" mb={ 4 }>FAQ</Heading>
      <AccordionRoot variant="faq">
        { FAQ_ITEMS.map((item, index) => (
          <GasTrackerFaqItem key={ index } question={ item.question } answer={ item.answer }/>
        )) }
      </AccordionRoot>
    </Box>
  );
};

export default GasTrackerFaq;
