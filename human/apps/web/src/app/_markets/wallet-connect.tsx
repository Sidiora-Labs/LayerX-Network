"use client";

import { usePathname, useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { connectWalletAccount } from "../../api/wallet";
import { KitButton } from "../../kit/control";
import { CopyableIdentifier } from "../../kit/money";
import { InlineNotice } from "../../kit/surface";

export function WalletConnect({ account }: Readonly<{ account: string | undefined }>) {
  const router = useRouter();
  const pathname = usePathname();
  const [pending, startTransition] = useTransition();
  const [problem, setProblem] = useState<string | undefined>(undefined);

  const connect = () => {
    setProblem(undefined);
    startTransition(async () => {
      try {
        const address = await connectWalletAccount();
        if (address === undefined) {
          setProblem("No Paxeer wallet answered. Open this page in a browser with the Paxeer wallet.");
          return;
        }
        router.replace(`${pathname}?account=${address}`);
      } catch {
        setProblem("The wallet did not share an account.");
      }
    });
  };

  return (
    <div className="flex flex-col gap-2">
      {account === undefined ? (
        <KitButton variant="secondary" loading={pending} onClick={connect}>
          Connect wallet
        </KitButton>
      ) : (
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <CopyableIdentifier label="Account" value={account} />
          <KitButton variant="ghost" size="sm" loading={pending} onClick={connect}>
            Switch
          </KitButton>
        </div>
      )}
      {problem === undefined ? null : <InlineNotice tone="warning">{problem}</InlineNotice>}
    </div>
  );
}
