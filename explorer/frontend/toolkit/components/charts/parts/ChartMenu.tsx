import { Icon } from '@chakra-ui/react';
import { useCopyToClipboard } from '@uidotdev/usehooks';
import dayjs from 'dayjs';
import domToImage from 'dom-to-image';
import React from 'react';

import type { Resolution, TimeChartData } from '../types';

import CopyIcon from 'icons/copy.svg';
import DotsIcon from 'icons/dots.svg';
import CsvIcon from 'icons/files/csv.svg';
import ImageIcon from 'icons/files/image.svg';
import ScopeIcon from 'icons/scope.svg';
import ShareIcon from 'icons/share.svg';
import { useMultichainContext } from 'lib/contexts/multichain';

import { useColorModeValue } from '../../../chakra/color-mode';
import { IconButton } from '../../../chakra/icon-button';
import { MenuContent, MenuItem, MenuRoot, MenuTrigger } from '../../../chakra/menu';
import { useDisclosure } from '../../../hooks/useDisclosure';
import { saveAsCsv } from '../../../utils/file';
import { isBrowser } from '../../../utils/isBrowser';
import ChartFullscreenDialog from '../ChartFullscreenDialog';

export const CHART_MENU_ITEMS_IDS = [
  'share' as const,
  'fullscreen' as const,
  'save_png' as const,
  'save_csv' as const,
];

export type ChartMenuItemId = (typeof CHART_MENU_ITEMS_IDS)[number];

export interface Props {
  itemIds?: Array<ChartMenuItemId>;
  charts: TimeChartData;
  title: string;
  description?: string;
  isLoading: boolean;
  chartRef: React.RefObject<HTMLDivElement | null>;
  chartUrl?: string;
  resolution?: Resolution;
  zoomRange?: [ Date, Date ];
  handleZoom: (range: [ Date, Date ]) => void;
  handleZoomReset: () => void;
};

const DOWNLOAD_IMAGE_SCALE = 5;

const ChartMenu = ({
  itemIds = CHART_MENU_ITEMS_IDS,
  charts,
  title,
  description,
  isLoading,
  chartRef,
  chartUrl,
  resolution,
  zoomRange,
  handleZoom,
  handleZoomReset,
}: Props) => {
  const pngBackgroundColor = useColorModeValue('white', 'black');
  const fullscreenDialog = useDisclosure();

  const [ , copyToClipboard ] = useCopyToClipboard();

  const isInBrowser = isBrowser();

  const multichainContext = useMultichainContext();

  const chainPostfix = React.useMemo(() => {
    return multichainContext?.chain.name ? ` on ${ multichainContext.chain.name }` : '';
  }, [ multichainContext?.chain.name ]);

  const showChartFullscreen = React.useCallback(() => {
    fullscreenDialog.onOpenChange({ open: true });
  }, [ fullscreenDialog ]);

  const handleFileSaveClick = React.useCallback(() => {
    // wait for context menu to close
    setTimeout(() => {
      if (chartRef.current) {
        domToImage.toPng(chartRef.current,
          {
            quality: 100,
            bgcolor: pngBackgroundColor,
            width: chartRef.current.offsetWidth * DOWNLOAD_IMAGE_SCALE,
            height: chartRef.current.offsetHeight * DOWNLOAD_IMAGE_SCALE,
            filter: (node) => node.nodeName !== 'BUTTON',
            style: {
              borderColor: 'transparent',
              transform: `scale(${ DOWNLOAD_IMAGE_SCALE })`,
              'transform-origin': 'top left',
            },
          })
          .then((dataUrl) => {
            const link = document.createElement('a');
            link.download = `${ title }${ chainPostfix } (Blockscout chart).png`;
            link.href = dataUrl;
            link.click();
            link.remove();
          });
      }
    }, 100);
  }, [ pngBackgroundColor, title, chainPostfix, chartRef ]);

  const handleSVGSavingClick = React.useCallback(() => {
    const headerRows = [
      'Date', ...charts.map((chart) => chart.name),
    ];
    const dataRows = charts[0].items.map((item, index) => [
      item.dateLabel ?? dayjs(item.date).format('YYYY-MM-DD'),
      ...charts.map((chart) => String(chart.items[index].value)),
    ]);
    saveAsCsv(headerRows, dataRows, `${ title }${ chainPostfix } (Blockscout stats)`);
  }, [ charts, title, chainPostfix ]);

  // TS thinks window.navigator.share can't be undefined, but it can
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const hasShare = isInBrowser && (window.navigator.share as any);

  const handleCopy = React.useCallback(() => {
    copyToClipboard(chartUrl ?? '');
  }, [ chartUrl, copyToClipboard ]);

  const handleShare = React.useCallback(async() => {
    try {
      await window.navigator.share({
        title: title,
        text: description,
        url: chartUrl,
      });
    } catch (error) {}
  }, [ title, description, chartUrl ]);

  return (
    <>
      <MenuRoot>
        <MenuTrigger asChild>
          <IconButton variant="icon_background" size="md" aria-label="Open chart options menu" loadingSkeleton={ isLoading }>
            <Icon><DotsIcon/></Icon>
          </IconButton>
        </MenuTrigger>
        <MenuContent>
          { itemIds.includes('share') && chartUrl && (
            <MenuItem
              value={ hasShare ? 'share' : 'copy' }
              onClick={ hasShare ? handleShare : handleCopy }
              closeOnSelect={ hasShare ? false : true }
            >
              <Icon boxSize={ 5 }>{ hasShare ? <ShareIcon/> : <CopyIcon/> }</Icon>
              { hasShare ? 'Share' : 'Copy link' }
            </MenuItem>
          ) }
          { itemIds.includes('fullscreen') && (
            <MenuItem
              value="fullscreen"
              onClick={ showChartFullscreen }
            >
              <Icon boxSize={ 5 }><ScopeIcon/></Icon>
              View fullscreen
            </MenuItem>
          ) }
          { itemIds.includes('save_png') && (
            <MenuItem
              value="save-png"
              onClick={ handleFileSaveClick }
            >
              <Icon boxSize={ 5 }><ImageIcon/></Icon>
              Save as PNG
            </MenuItem>
          ) }
          { itemIds.includes('save_csv') && (
            <MenuItem
              value="save-csv"
              onClick={ handleSVGSavingClick }
            >
              <Icon boxSize={ 5 }><CsvIcon/></Icon>
              Save as CSV
            </MenuItem>
          ) }
        </MenuContent>
      </MenuRoot>
      <ChartFullscreenDialog
        open={ fullscreenDialog.open }
        onOpenChange={ fullscreenDialog.onOpenChange }
        charts={ charts }
        title={ title }
        description={ description }
        resolution={ resolution }
        zoomRange={ zoomRange }
        handleZoom={ handleZoom }
        handleZoomReset={ handleZoomReset }
      />
    </>
  );
};

export default ChartMenu;
