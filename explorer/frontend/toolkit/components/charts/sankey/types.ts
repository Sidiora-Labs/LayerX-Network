import type { SankeyNode, SankeyLink } from 'd3-sankey';

export interface SankeyNodeDatum {
  readonly id: string;
  readonly name: string;
}

export interface SankeyLinkDatum {
  readonly source: string;
  readonly target: string;
  readonly value: number;
}

export interface SankeyData {
  readonly nodes: ReadonlyArray<SankeyNodeDatum>;
  readonly links: ReadonlyArray<SankeyLinkDatum>;
}

// After d3-sankey layout, nodes are the input datum merged with the layout properties.
export type SankeyNodeExtended = SankeyNodeDatum & SankeyNode<SankeyNodeDatum, SankeyLinkDatum>;

// After d3-sankey layout, source/target are always resolved node objects, not ids.
export type SankeyLinkExtended = Omit<SankeyLink<SankeyNodeDatum, SankeyLinkDatum>, 'source' | 'target'> & {
  source: SankeyNodeExtended;
  target: SankeyNodeExtended;
};
