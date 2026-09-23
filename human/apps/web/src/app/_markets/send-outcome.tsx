import type { PrecompileSendOutcome } from "../../api/wallet";
import { InlineNotice } from "../../kit/surface";

export function SendOutcome({ outcome }: Readonly<{ outcome: PrecompileSendOutcome | undefined }>) {
  if (outcome === undefined) {
    return null;
  }
  switch (outcome.outcome) {
    case "sent":
      return <InlineNotice tone="success">Sent. Transaction {outcome.transactionHash}</InlineNotice>;
    case "cancelled":
      return <InlineNotice tone="neutral">You cancelled in the wallet.</InlineNotice>;
    case "rejected":
      return <InlineNotice tone="danger">The wallet refused this account.</InlineNotice>;
    case "unavailable":
      return <InlineNotice tone="warning">No Paxeer wallet is connected.</InlineNotice>;
    case "failed":
      return (
        <InlineNotice tone="danger" role="alert">
          The transaction was not sent{outcome.detail === undefined ? "." : `: ${outcome.detail}`}
        </InlineNotice>
      );
  }
}
