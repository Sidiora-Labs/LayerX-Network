export const TEXT_LINE_HEIGHT = 12;
export const PADDING = 16;
export const LINE_SPACE = 10;
export const POINT_SIZE = 16;
export const LABEL_WIDTH = 80;

export const calculateContainerHeight = (seriesNum: number, isIncomplete?: boolean) => {
  const linesNum = isIncomplete ? seriesNum + 2 : seriesNum + 1;

  return 2 * PADDING + linesNum * TEXT_LINE_HEIGHT + (linesNum - 1) * LINE_SPACE;
};

export const calculateRowTransformValue = (rowNum: number) => {
  const top = Math.max(0, PADDING + rowNum * (LINE_SPACE + TEXT_LINE_HEIGHT));
  return `translate(${ PADDING },${ top })`;
};
