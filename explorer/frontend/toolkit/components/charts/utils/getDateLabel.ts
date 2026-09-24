import * as d3 from 'd3';

import { Resolution } from '../types';

export function getDateLabel(date: Date, dateTo?: Date, resolution?: Resolution): string {
  switch (resolution) {
    case Resolution.WEEK:
      return d3.utcFormat('%e %b %Y')(date) + (dateTo ? ` – ${ d3.utcFormat('%e %b %Y')(dateTo) }` : '');
    case Resolution.MONTH:
      return d3.utcFormat('%b %Y')(date);
    case Resolution.YEAR:
      return d3.utcFormat('%Y')(date);
    default:
      return d3.utcFormat('%e %b %Y')(date);
  }
}
