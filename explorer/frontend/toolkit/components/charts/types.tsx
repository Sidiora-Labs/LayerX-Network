import type * as d3 from 'd3';

export { Resolution } from '@blockscout/stats-types';
import { Resolution as ResolutionEnum } from '@blockscout/stats-types';

export const RESOLUTION_LABELS: Array<{ id: ResolutionEnum; title: string }> = [
  {
    id: ResolutionEnum.DAY,
    title: 'Day',
  },
  {
    id: ResolutionEnum.WEEK,
    title: 'Week',
  },
  {
    id: ResolutionEnum.MONTH,
    title: 'Month',
  },
  {
    id: ResolutionEnum.YEAR,
    title: 'Year',
  },
];

export interface TimeChartItemRaw {
  date: Date;
  dateLabel?: string;
  value: number | string | null;
}

export interface TimeChartItem {
  date: Date;
  date_to?: Date;
  dateLabel?: string;
  value: number;
  isApproximate?: boolean;
}

export interface ChartMargin {
  top?: number;
  right?: number;
  bottom?: number;
  left?: number;
}

export interface ChartOffset {
  x?: number;
  y?: number;
}

export type ChartConfig =
  | {
    type: 'line';
    color: string;
    strokeWidth?: number;
    strokeDasharray?: string;
  } |
  {
    type: 'area';
    gradient: {
      startColor: string;
      stopColor: string;
    };
  };

export interface TimeChartDataItem {
  id: string;
  name: string;
  items: Array<TimeChartItem>;
  charts: Array<ChartConfig>;
  units?: string;
  valueFormatter?: (value: number) => string;
}

export type TimeChartData = Array<TimeChartDataItem>;

export interface AxisConfig {
  ticks?: number;
  nice?: boolean;
  noLabel?: boolean;
  scale?: {
    min?: number;
  };
  tickFormatter?: () => (d: d3.AxisDomain) => string;
}

export interface AxesConfig {
  x?: AxisConfig;
  y?: AxisConfig;
}

export type AxesConfigFn = (props: {
  isEnlarged?: boolean;
  isMobile?: boolean;
}) => AxesConfig;
