import type { ReactNode } from "react";

import { InlineNotice, ScreenCard } from "../../kit/surface";
import { MarketNavigation } from "./navigation";
import { WalletConnect } from "./wallet-connect";

export function MarketFrame({
  title,
  description,
  account,
  children,
}: Readonly<{ title: string; description: string; account: string | undefined; children: ReactNode }>) {
  return (
    <ScreenCard title={title} description={description}>
      <div className="flex flex-col gap-4">
        <MarketNavigation account={account} />
        <WalletConnect account={account} />
        {children}
      </div>
    </ScreenCard>
  );
}

export function MarketUnavailable({ detail }: Readonly<{ detail: string }>) {
  return (
    <InlineNotice tone="warning" role="alert">
      {detail}
    </InlineNotice>
  );
}
