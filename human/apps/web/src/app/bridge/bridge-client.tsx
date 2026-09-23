"use client";

import { useRouter } from "next/navigation";
import { useEffect, useState, useTransition } from "react";

import { bridgeOutCall } from "../../api/sdk";
import { sendWalletPrecompileCall, type PrecompileSendOutcome } from "../../api/wallet";
import { KitButton } from "../../kit/control";
import { TextField } from "../../kit/field";
import { availability } from "../_markets/availability";
import { isBytes32, isEvmAddress } from "../_markets/format";
import { SendOutcome } from "../_markets/send-outcome";

const POLL_INTERVAL_MS = 15_000;
const INTEGER = /^[1-9][0-9]{0,77}$/u;
const LOG_INDEX = /^(0|[1-9][0-9]{0,18})$/u;

/** Re-reads the server-rendered attestation status while a tracked deposit is still waiting. */
export function BridgeStatusPolling({ active }: Readonly<{ active: boolean }>) {
  const router = useRouter();
  useEffect(() => {
    if (!active) {
      return undefined;
    }
    const timer = window.setInterval(() => {
      router.refresh();
    }, POLL_INTERVAL_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [active, router]);
  return active ? <p className="text-sm text-muted-foreground">Checking again every 15 seconds.</p> : null;
}

export function BridgeTrackForm({
  account,
  chain,
  asset,
}: Readonly<{ account: string | undefined; chain: string; asset: string | undefined }>) {
  const router = useRouter();
  const [txHash, setTxHash] = useState("");
  const [logIndex, setLogIndex] = useState("");
  const ready = isBytes32(txHash) && LOG_INDEX.test(logIndex);
  return (
    <div className="flex flex-col gap-3">
      <TextField
        label="Ethereum deposit transaction hash"
        value={txHash}
        spellCheck={false}
        onChange={(event) => {
          setTxHash(event.target.value.trim().toLowerCase());
        }}
      />
      <TextField
        label="BridgeDeposit log index"
        inputMode="numeric"
        value={logIndex}
        onChange={(event) => {
          setLogIndex(event.target.value.trim());
        }}
      />
      <KitButton
        variant="secondary"
        onClick={() => {
          const parameters = new URLSearchParams({ chain, tx: txHash, log: logIndex });
          if (account !== undefined) {
            parameters.set("account", account);
          }
          if (asset !== undefined) {
            parameters.set("asset", asset);
          }
          router.replace(`/bridge?${parameters.toString()}`);
        }}
        {...availability(!ready && "Enter the transaction hash and log index.")}
      >
        Track deposit
      </KitButton>
    </div>
  );
}

export function BridgeOutForm({
  account,
  chain,
  asset,
  closedReason,
  maxPerTx,
  headroom,
}: Readonly<{
  account: string | undefined;
  chain: string;
  asset: string | undefined;
  closedReason: string | undefined;
  maxPerTx: string | undefined;
  headroom: string | undefined;
}>) {
  const router = useRouter();
  const [pending, startTransition] = useTransition();
  const [amountText, setAmountText] = useState("");
  const [recipient, setRecipient] = useState(account ?? "");
  const [outcome, setOutcome] = useState<PrecompileSendOutcome | undefined>(undefined);
  const amount = INTEGER.test(amountText.trim()) ? BigInt(amountText.trim()) : undefined;
  const overCap =
    amount !== undefined &&
    ((maxPerTx !== undefined && amount > BigInt(maxPerTx)) || (headroom !== undefined && amount > BigInt(headroom)));

  return (
    <div className="flex flex-col gap-3">
      <TextField
        label="Amount (base units)"
        inputMode="numeric"
        value={amountText}
        onChange={(event) => {
          setAmountText(event.target.value);
        }}
        errorMessage={overCap ? "This is above the per-transfer cap or the in-flight headroom." : undefined}
      />
      <TextField
        label="Ethereum recipient"
        value={recipient}
        spellCheck={false}
        onChange={(event) => {
          setRecipient(event.target.value.trim());
        }}
      />
      <KitButton
        loading={pending}
        onClick={() => {
          if (account === undefined || asset === undefined || amount === undefined) {
            return;
          }
          startTransition(async () => {
            const sent = await sendWalletPrecompileCall(account, () => bridgeOutCall(BigInt(chain), asset, amount, recipient));
            setOutcome(sent);
            if (sent.outcome === "sent") {
              router.refresh();
            }
          });
        }}
        {...availability(
          account === undefined && "Connect a wallet first.",
          closedReason,
          (amount === undefined || !isEvmAddress(recipient)) && "Enter an amount and an Ethereum recipient.",
          overCap && "The amount is above the bridge caps.",
        )}
      >
        Bridge out
      </KitButton>
      <SendOutcome outcome={outcome} />
    </div>
  );
}
