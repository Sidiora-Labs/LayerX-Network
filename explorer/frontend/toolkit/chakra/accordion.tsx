import { Accordion, Icon } from '@chakra-ui/react';
import * as React from 'react';
import { scroller } from 'react-scroll';

import IndicatorIcon from 'icons/arrows/east-mini.svg';

interface AccordionItemTriggerProps extends Accordion.ItemTriggerProps {
  indicatorPlacement?: 'start' | 'end';
  noIndicator?: boolean;
  variant?: Accordion.RootProps['variant'];
}

export const AccordionItemTrigger = React.forwardRef<
  HTMLButtonElement,
  AccordionItemTriggerProps
>(function AccordionItemTrigger(props, ref) {
  const { children, indicatorPlacement: indicatorPlacementProp, variant, noIndicator, ...rest } = props;

  const indicatorPlacement = variant === 'faq' ? 'start' : (indicatorPlacementProp ?? 'end');

  const indicator = variant === 'faq' ? (
    <Accordion.ItemIndicator
      asChild
      rotate="0deg"
      position="relative"
      _before={{
        content: '""',
        position: 'absolute',
        display: 'block',
        bgColor: '{currentColor}',
        w: '100%',
        h: '2px',
        borderRadius: '2px',
        left: '0',
        top: '50%',
        transform: 'translateY(-50%)',
      }}
      _after={{
        content: '""',
        position: 'absolute',
        display: 'block',
        bgColor: '{currentColor}',
        w: '2px',
        h: '100%',
        borderRadius: '2px',
        left: '50%',
        top: '0',
        transform: 'translateX(-50%)',
        transition: 'transform 0.2s ease-in-out',
      }}
      _open={{
        _after: {
          transform: 'translateX(-50%) rotate(90deg)',
        },
      }}
    >
      <div/>
    </Accordion.ItemIndicator>
  ) : (
    <Accordion.ItemIndicator rotate={{ base: '180deg', _open: '270deg' }} display="flex">
      <Icon boxSize={ 5 }><IndicatorIcon/></Icon>
    </Accordion.ItemIndicator>
  );

  return (
    <Accordion.ItemTrigger className="group" { ...rest } ref={ ref }>
      { indicatorPlacement === 'start' && !noIndicator && indicator }
      { children }
      { indicatorPlacement === 'end' && !noIndicator && indicator }
    </Accordion.ItemTrigger>
  );
});

export interface AccordionItemContentProps extends Accordion.ItemContentProps {}

export const AccordionItemContent = React.forwardRef<
  HTMLDivElement,
  AccordionItemContentProps
>(function AccordionItemContent(props, ref) {
  return (
    <Accordion.ItemContent>
      <Accordion.ItemBody { ...props } ref={ ref }/>
    </Accordion.ItemContent>
  );
});

export const AccordionRoot = (props: Accordion.RootProps) => {
  const { multiple = true, ...rest } = props;
  return <Accordion.Root multiple={ multiple } { ...rest }/>;
};

export const AccordionItem = Accordion.Item;

export function useAccordion(items: Array<{ id: string }>) {
  const [ value, setValue ] = React.useState<Array<string>>([]);

  const onValueChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setValue(value);
  }, []);

  const scrollToItemFromUrl = React.useCallback(() => {
    const hash = window.location.hash.replace('#', '');

    if (!hash) {
      return;
    }

    const itemToScroll = items.find((item) => item.id === hash);
    if (itemToScroll) {
      scroller.scrollTo(itemToScroll.id, {
        duration: 500,
        smooth: true,
        offset: -100,
      });
      setValue([ itemToScroll.id ]);
    }
  }, [ items ]);

  return React.useMemo(() => {
    return {
      value,
      onValueChange,
      scrollToItemFromUrl,
    };
  }, [ value, onValueChange, scrollToItemFromUrl ]);
}
