export interface UptimeRealTimeData {
  end_block: number;
  instant_mgas_per_second: number;
  instant_mini_block_interval: number;
  instant_tps: number;
  latest_mini_block_id: number;
  updated_at: string;
}

export interface UptimeHistoryItem {
  value: number;
  timestamp: number;
}

export interface UptimeHistoryFull {
  historical_gas_per_second_3h: Array<UptimeHistoryItem>;
  historical_gas_per_second_7d: Array<UptimeHistoryItem>;
  historical_gas_per_second_24h: Array<UptimeHistoryItem>;
  historical_tps_3h: Array<UptimeHistoryItem>;
  historical_tps_7d: Array<UptimeHistoryItem>;
  historical_tps_24h: Array<UptimeHistoryItem>;
  historical_mini_block_interval_3h: Array<UptimeHistoryItem>;
  historical_mini_block_interval_7d: Array<UptimeHistoryItem>;
  historical_mini_block_interval_24h: Array<UptimeHistoryItem>;
}

export type UptimeSocketData = {
  realtime: UptimeRealTimeData;
  timestamp: number;
  type: 'realtime_metrics';
} | {
  data: UptimeHistoryFull;
  type: 'history_full';
} | {
  data: Partial<Record<keyof UptimeHistoryFull, UptimeHistoryItem>>;
  type: 'history_delta';
};
