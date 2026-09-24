import * as d3 from 'd3';
import { maxBy, uniq } from 'es-toolkit';

import type { AxesConfig, AxisConfig, TimeChartData } from '../types';

import { DAY, MONTH, YEAR } from '../../../utils/consts';

export const DEFAULT_MAXIMUM_SIGNIFICANT_DIGITS = 2;
export const DEFAULT_MAXIMUM_FRACTION_DIGITS = 3;
export const MAXIMUM_SIGNIFICANT_DIGITS_LIMIT = 8;
export const DEFAULT_LABEL_LENGTH = 5;

export interface LabelFormatParams extends Intl.NumberFormatOptions {
  maxLabelLength: number;
}

type Data = TimeChartData;

export function getAxesParams(data: Data, axesConfig?: AxesConfig) {
  const { labelFormatParams: labelFormatParamsY, scale: yScale } = getAxisParamsY(data, axesConfig?.y, axesConfig?.y?.tickFormatter);

  return {
    x: {
      scale: getAxisParamsX(data).scale,
      tickFormatter: axesConfig?.x?.tickFormatter ?? tickFormatterX,
    },
    y: {
      scale: yScale,
      labelFormatParams: labelFormatParamsY,
      tickFormatter: axesConfig?.y?.tickFormatter ?? getTickFormatterY(labelFormatParamsY),
    },
  };
}

function getAxisParamsX(data: Data) {
  const min = d3.min(data, ({ items }) => d3.min(items, ({ date }) => date)) ?? new Date();
  const max = d3.max(data, ({ items }) => d3.max(items, ({ date }) => date)) ?? new Date();
  const scale = d3.scaleTime().domain([ min, max ]);

  return { min, max, scale };
}

const tickFormatterX = (axis: d3.Axis<d3.NumberValue>) => (d: d3.AxisDomain) => {
  let format: (date: Date) => string;
  const scale = axis.scale();
  const extent = scale.domain();

  const span = Number(extent[1]) - Number(extent[0]);

  if (span > 2 * YEAR) {
    format = d3.utcFormat('%Y');
  } else if (span > 4 * MONTH) {
    format = d3.utcFormat('%b \'%y');
  } else if (span > 2 * DAY) {
    format = d3.utcFormat('%d %b');
  } else {
    format = d3.utcFormat('%H:%M');
  }

  return format(d as Date);
};

function getAxisParamsY(data: Data, config?: AxisConfig, tickFormatter?: () => (d: d3.AxisDomain) => string) {
  const DEFAULT_TICKS_NUM = 3;
  const min = d3.min(data, ({ items }) => d3.min(items, ({ value }) => value)) ?? 0;
  const max = d3.max(data, ({ items }) => d3.max(items, ({ value }) => value)) ?? 0;
  const scale = config?.nice ?
    d3.scaleLinear()
      .domain([ config?.scale?.min ?? min, max ])
      .nice(config?.ticks ?? DEFAULT_TICKS_NUM) :
    d3.scaleLinear()
      .domain([ config?.scale?.min ?? min, max ]);

  const ticks = scale.ticks(config?.ticks ?? DEFAULT_TICKS_NUM);
  const labelFormatParams = getYLabelFormatParams(ticks, tickFormatter);

  return { min, max, scale, labelFormatParams };
}

const getTickFormatterY = (params: Intl.NumberFormatOptions) => () => (d: d3.AxisDomain) => {
  const num = Number(d);
  return num.toLocaleString(undefined, params);
};

function getYLabelFormatParams(
  ticks: Array<number>,
  tickFormatter?: () => (d: d3.AxisDomain) => string,
  maximumSignificantDigits = DEFAULT_MAXIMUM_SIGNIFICANT_DIGITS,
): LabelFormatParams {
  const params = {
    maximumFractionDigits: DEFAULT_MAXIMUM_FRACTION_DIGITS,
    maximumSignificantDigits,
    notation: 'compact' as const,
  };

  const uniqTicksStr = uniq(ticks.map((tick) => tickFormatter ? tickFormatter()(tick) : tick.toLocaleString(undefined, params)));
  const maxLabelLength = maxBy(uniqTicksStr, (items) => items.length)?.length ?? DEFAULT_LABEL_LENGTH;

  if (uniqTicksStr.length === ticks.length || maximumSignificantDigits === MAXIMUM_SIGNIFICANT_DIGITS_LIMIT) {
    return { ...params, maxLabelLength };
  }

  return getYLabelFormatParams(ticks, tickFormatter, maximumSignificantDigits + 1);
}
