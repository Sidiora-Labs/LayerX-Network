import type { StackProps } from '@chakra-ui/react';
import { createListCollection, HStack } from '@chakra-ui/react';
import React from 'react';

import { useSettingsContext } from 'lib/contexts/settings';
import dayjs from 'lib/date/dayjs';
import { IconButton } from 'toolkit/chakra/icon-button';
import type { SelectOption } from 'toolkit/chakra/select';
import { SelectContent, SelectItem, SelectRoot, SelectControl } from 'toolkit/chakra/select';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import IconSvg from 'ui/shared/IconSvg';
import TextSeparator from 'ui/shared/TextSeparator';

const FORMAT_OPTIONS = [
  { label: 'Local', value: 'local' as const },
  { label: 'UTC', value: 'utc' as const },
  { label: 'Unix', value: 'unix' as const },
];

const collection = createListCollection<SelectOption>({
  items: FORMAT_OPTIONS,
});

type Format = (typeof FORMAT_OPTIONS)[number]['value'];

interface Props extends StackProps {
  timestamp: string | number;
  isLoading?: boolean;
  noRelativeTime?: boolean;
};

const DetailedInfoTimestamp = ({ timestamp, isLoading, noRelativeTime, ...rest }: Props) => {

  const settings = useSettingsContext();

  const [ format, setFormat ] = React.useState<Array<Format>>(settings?.isLocalTime === false ? [ 'utc' ] : [ 'local' ]);

  const handleFormatChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setFormat(value as Array<Format>);
  }, []);

  const timeText = (() => {
    if (format.includes('local')) {
      return dayjs(timestamp).format('llll');
    }

    if (format.includes('utc')) {
      return dayjs(timestamp).utc().format('MMM DD YYYY HH:mm:ss');
    }

    return dayjs(timestamp).unix().toString();
  })();

  return (
    <HStack maxW="100%" minW="0" { ...rest }>
      { !noRelativeTime && (
        <>
          <Skeleton loading={ isLoading } flexShrink={ 0 }>
            { dayjs(timestamp).fromNow() }
          </Skeleton>
          <TextSeparator mx={ 0 }/>
        </>
      ) }
      <HStack gap={ 1 } minW="0">
        <SelectRoot
          name="time_format"
          collection={ collection }
          defaultValue={ format }
          onValueChange={ handleFormatChange }
          variant="plain"
          w="fit-content"
        >
          <SelectControl
            triggerProps={{ asChild: true, minH: 'auto', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
            noIndicator
            defaultValue={ format }
          >
            <IconButton
              aria-label="Toggle time format"
              variant="icon_secondary"
              boxSize={ 5 }
              borderRadius="sm"
              selected={ !isLoading }
              loadingSkeleton={ isLoading }
            >
              <IconSvg name="clock-light" boxSize="14px"/>
            </IconButton>
          </SelectControl>
          <SelectContent>
            { collection.items.map((item) => (
              <SelectItem item={ item } key={ item.value }>
                { item.label }
              </SelectItem>
            )) }
          </SelectContent>
        </SelectRoot>
        <TruncatedText
          text={ timeText }
          loading={ isLoading }
          minW="0"
        />
      </HStack>
    </HStack>
  );
};

export default DetailedInfoTimestamp;
