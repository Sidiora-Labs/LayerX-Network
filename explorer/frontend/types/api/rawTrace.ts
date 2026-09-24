export interface RawTrace {
  action: {
    callType: string;
    from: string;
    gas: string;
    input: string;
    to: string;
    value: string;
  };
  result: {
    gasUsed: string;
    output: string;
  };
  error: string | null;
  subtraces: number;
  traceAddress: Array<number>;
  type: string;
}

export type RawTracesResponse = Array<RawTrace>;
