import React from 'react';

import useIsMobile from 'lib/hooks/useIsMobile';
import { useColorMode } from 'toolkit/chakra/color-mode';
import { Switch } from 'toolkit/chakra/switch';
import { TabsList, TabsRoot, TabsTrigger } from 'toolkit/chakra/tabs';
import PageTitle from 'ui/shared/Page/PageTitle';
import AccordionsShowcase from 'ui/showcases/Accordion';
import AlertShowcase from 'ui/showcases/Alert';
import BadgeShowcase from 'ui/showcases/Badge';
import ButtonShowcase from 'ui/showcases/Button';
import CheckboxShowcase from 'ui/showcases/Checkbox';
import ClipboardShowcase from 'ui/showcases/Clipboard';
import CloseButtonShowcase from 'ui/showcases/CloseButton';
import CollapsibleShowcase from 'ui/showcases/Collapsible';
import ContentLoaderShowcase from 'ui/showcases/ContentLoader';
import DialogShowcase from 'ui/showcases/Dialog';
import EmptyStateShowcase from 'ui/showcases/EmptyState';
import FieldShowcase from 'ui/showcases/Field';
import IconButtonShowcase from 'ui/showcases/IconButton';
import InputShowcase from 'ui/showcases/Input';
import LinkShowcase from 'ui/showcases/Link';
import MenuShowcase from 'ui/showcases/Menu';
import PaginationShowcase from 'ui/showcases/Pagination';
import PinInputShowcase from 'ui/showcases/PinInput';
import PopoverShowcase from 'ui/showcases/Popover';
import ProgressShowcase from 'ui/showcases/Progress';
import ProgressCircleShowcase from 'ui/showcases/ProgressCircle';
import RadioShowcase from 'ui/showcases/Radio';
import RatingShowcase from 'ui/showcases/Rating';
import SelectShowcase from 'ui/showcases/Select';
import SkeletonShowcase from 'ui/showcases/Skeleton';
import SpinnerShowcase from 'ui/showcases/Spinner';
import SwitchShowcase from 'ui/showcases/Switch';
import TableShowcase from 'ui/showcases/Table';
import TabsShowcase from 'ui/showcases/Tabs';
import TagShowcase from 'ui/showcases/Tag';
import TextareaShowcase from 'ui/showcases/Textarea';
import ToastShowcase from 'ui/showcases/Toast';
import TooltipShowcase from 'ui/showcases/Tooltip';
import ValuesShowcase from 'ui/showcases/Values';

const tabs = [
  { label: 'Accordion', value: 'accordion', component: <AccordionsShowcase/> },
  { label: 'Alert', value: 'alert', component: <AlertShowcase/> },
  { label: 'Badge', value: 'badge', component: <BadgeShowcase/> },
  { label: 'Button', value: 'button', component: <ButtonShowcase/> },
  { label: 'Checkbox', value: 'checkbox', component: <CheckboxShowcase/> },
  { label: 'Clipboard', value: 'clipboard', component: <ClipboardShowcase/> },
  { label: 'Close button', value: 'close-button', component: <CloseButtonShowcase/> },
  { label: 'Collapsible', value: 'collapsible', component: <CollapsibleShowcase/> },
  { label: 'Content loader', value: 'content-loader', component: <ContentLoaderShowcase/> },
  { label: 'Dialog', value: 'dialog', component: <DialogShowcase/> },
  { label: 'Empty state', value: 'empty-state', component: <EmptyStateShowcase/> },
  { label: 'Field', value: 'field', component: <FieldShowcase/> },
  { label: 'Icon button', value: 'icon-button', component: <IconButtonShowcase/> },
  { label: 'Input', value: 'input', component: <InputShowcase/> },
  { label: 'Link', value: 'link', component: <LinkShowcase/> },
  { label: 'Menu', value: 'menu', component: <MenuShowcase/> },
  { label: 'Pagination', value: 'pagination', component: <PaginationShowcase/> },
  { label: 'Progress', value: 'progress', component: <ProgressShowcase/> },
  { label: 'Progress Circle', value: 'progress-circle', component: <ProgressCircleShowcase/> },
  { label: 'Radio', value: 'radio', component: <RadioShowcase/> },
  { label: 'Rating', value: 'rating', component: <RatingShowcase/> },
  { label: 'Pin input', value: 'pin-input', component: <PinInputShowcase/> },
  { label: 'Popover', value: 'popover', component: <PopoverShowcase/> },
  { label: 'Select', value: 'select', component: <SelectShowcase/> },
  { label: 'Skeleton', value: 'skeleton', component: <SkeletonShowcase/> },
  { label: 'Spinner', value: 'spinner', component: <SpinnerShowcase/> },
  { label: 'Switch', value: 'switch', component: <SwitchShowcase/> },
  { label: 'Table', value: 'table', component: <TableShowcase/> },
  { label: 'Tabs', value: 'tabs', component: <TabsShowcase/> },
  { label: 'Tag', value: 'tag', component: <TagShowcase/> },
  { label: 'Textarea', value: 'textarea', component: <TextareaShowcase/> },
  { label: 'Toast', value: 'toast', component: <ToastShowcase/> },
  { label: 'Tooltip', value: 'tooltip', component: <TooltipShowcase/> },
  { label: 'Values', value: 'values', component: <ValuesShowcase/> },
];

const ChakraShowcases = () => {
  const colorMode = useColorMode();
  const isMobile = useIsMobile();

  return (
    <>
      <PageTitle title="Chakra UI Showcase"/>
      <Switch onCheckedChange={ colorMode.toggleColorMode } checked={ colorMode.colorMode === 'dark' } mb={ 10 }>
        Color mode: { colorMode.colorMode }
      </Switch>

      <TabsRoot defaultValue="accordion" orientation={ isMobile ? 'horizontal' : 'vertical' }>
        <TabsList flexWrap="wrap" w="fit-content" whiteSpace="nowrap">
          { tabs.map((tab) => (
            <TabsTrigger key={ tab.value } value={ tab.value }>{ tab.label }</TabsTrigger>
          )) }
        </TabsList>
        { tabs.map((tab) => <React.Fragment key={ tab.value }>{ tab.component }</React.Fragment>) }
      </TabsRoot>
    </>
  );
};

export default React.memo(ChakraShowcases);
