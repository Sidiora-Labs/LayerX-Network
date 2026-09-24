import { nftHtmlEmbed } from './policies/nftHtmlEmbed';
import { makePolicyString } from './utils';

export default function generateNftHtmlEmbedCspPolicy(): string {
  return makePolicyString(nftHtmlEmbed());
}
