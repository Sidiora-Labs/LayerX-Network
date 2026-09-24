import { range } from 'es-toolkit';
import React from 'react';

export function useChartLegend(dataLength: number) {
  const [ selectedLines, setSelectedLines ] = React.useState<Array<number>>(range(dataLength));

  const handleLegendItemClick = React.useCallback((index: number) => {
    const nextSelectedLines = selectedLines.includes(index) ? selectedLines.filter((item) => item !== index) : [ ...selectedLines, index ];
    setSelectedLines(nextSelectedLines);
  }, [ selectedLines ]);

  return {
    selectedLines,
    handleLegendItemClick,
  };
}
