export interface Fields extends SocialLinkFields, TickerUrlFields {
  address: string;
  token_name: string;
  requester_name: string;
  requester_email: string;
  project_name?: string;
  project_sector: Array<string> | null;
  project_email: string;
  project_website: string;
  project_description: string;
  docs?: string;
  support?: string;
  icon_url: string;
  comment?: string;
}

export interface TickerUrlFields {
  ticker_coin_gecko?: string;
  ticker_coin_market_cap?: string;
  ticker_defi_llama?: string;
}

export interface SocialLinkFields {
  github?: string;
  telegram?: string;
  linkedin?: string;
  discord?: string;
  slack?: string;
  twitter?: string;
  opensea?: string;
  facebook?: string;
  medium?: string;
  reddit?: string;
}
