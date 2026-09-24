export type BannerFormat = 'mobile' | 'desktop' | 'responsive';

export interface BannerProps {
  className?: string;
  format?: BannerFormat;
}
