"use client";

import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import {
  exchangeCancelOrderCall,
  exchangeDepositMarginCall,
  exchangePlaceOrderCall,
  exchangeRequestSettlementCall,
  exchangeWithdrawMarginCall,
} from "../../api/sdk";
import { sendWalletPrecompileCall, type PrecompileSendOutcome } from "../../api/wallet";
import { SegmentedControl } from "../../kit/collection";
import { KitButton } from "../../kit/control";
import { ExplorerPanel } from "../../kit/explorer";
import { TextField } from "../../kit/field";
import { availability } from "../_markets/availability";
import { WEI_DECIMALS, isBytes32, parseUnits } from "../_markets/format";
import { ChoiceField } from "../_markets/select";
import { SendOutcome } from "../_markets/send-outcome";

const INTEGER = /^[1-9][0-9]{0,38}$/u;

function integer(text: string): bigint | undefined {
  return INTEGER.test(text.trim()) ? BigInt(text.trim()) : undefined;
}

export function ExchangeActions({
  account,
  layerxAccount,
  openOrders,
  timeInForce,
}: Readonly<{
  account: string | undefined;
  layerxAccount: string | undefined;
  openOrders: readonly Readonly<{ orderId: string; label: string }>[];
  timeInForce: readonly string[];
}>) {
  const router = useRouter();
  const [pending, startTransition] = useTransition();
  const [marketId, setMarketId] = useState("");
  const [side, setSide] = useState("1");
  const [priceText, setPriceText] = useState("");
  const [quantityText, setQuantityText] = useState("");
  const [tif, setTif] = useState("0");
  const [orderOutcome, setOrderOutcome] = useState<PrecompileSendOutcome | undefined>(undefined);
  const [cancelOrderId, setCancelOrderId] = useState(openOrders[0]?.orderId ?? "");
  const [cancelOutcome, setCancelOutcome] = useState<PrecompileSendOutcome | undefined>(undefined);
  const [depositText, setDepositText] = useState("");
  const [withdrawAsset, setWithdrawAsset] = useState("");
  const [withdrawText, setWithdrawText] = useState("");
  const [marginOutcome, setMarginOutcome] = useState<PrecompileSendOutcome | undefined>(undefined);
  const [positionId, setPositionId] = useState("");
  const [settleOutcome, setSettleOutcome] = useState<PrecompileSendOutcome | undefined>(undefined);

  const marginAccount = layerxAccount === undefined ? undefined : `0x${layerxAccount}`;
  const price = integer(priceText);
  const quantity = integer(quantityText);
  const deposit = parseUnits(depositText, WEI_DECIMALS);
  const withdraw = integer(withdrawText);
  const walletReason = account === undefined ? "Connect a wallet first." : undefined;
  const marginReason = marginAccount === undefined ? "This wallet has no bound LayerX account." : undefined;

  const send = (
    build: Parameters<typeof sendWalletPrecompileCall>[1],
    record: (outcome: PrecompileSendOutcome) => void,
  ) => {
    if (account === undefined) {
      return;
    }
    startTransition(async () => {
      const outcome = await sendWalletPrecompileCall(account, build);
      record(outcome);
      if (outcome.outcome === "sent") {
        router.refresh();
      }
    });
  };

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <ExplorerPanel title="Place an order">
        <div className="flex flex-col gap-3">
          <TextField
            label="Market id (bytes32)"
            value={marketId}
            spellCheck={false}
            onChange={(event) => {
              setMarketId(event.target.value.trim());
            }}
            errorMessage={marketId !== "" && !isBytes32(marketId) ? "A market id is 0x and 64 hex digits." : undefined}
          />
          <SegmentedControl
            aria-label="Side"
            value={side}
            onValueChange={setSide}
            options={[
              { value: "1", label: "Buy" },
              { value: "2", label: "Sell" },
            ]}
          />
          <TextField
            label="Price (market price units)"
            inputMode="numeric"
            value={priceText}
            onChange={(event) => {
              setPriceText(event.target.value);
            }}
          />
          <TextField
            label="Quantity (lots)"
            inputMode="numeric"
            value={quantityText}
            onChange={(event) => {
              setQuantityText(event.target.value);
            }}
          />
          <ChoiceField
            label="Time in force"
            value={tif}
            onChange={setTif}
            options={timeInForce.map((label, index) => ({ value: String(index), label }))}
          />
          <KitButton
            loading={pending}
            onClick={() => {
              if (price === undefined || quantity === undefined) {
                return;
              }
              send(
                () => exchangePlaceOrderCall({ marketId, side: Number(side), price, quantity, timeInForce: Number(tif) }),
                setOrderOutcome,
              );
            }}
            {...availability(
              walletReason,
              (!isBytes32(marketId) || price === undefined || quantity === undefined) && "Enter a market, price and quantity.",
            )}
          >
            Place order
          </KitButton>
          <SendOutcome outcome={orderOutcome} />
        </div>
      </ExplorerPanel>
      <ExplorerPanel title="Cancel an order">
        <div className="flex flex-col gap-3">
          {openOrders.length === 0 ? (
            <p className="text-sm text-muted-foreground">No order has a LayerX order id yet.</p>
          ) : (
            <ChoiceField
              label="Order"
              value={cancelOrderId}
              onChange={setCancelOrderId}
              options={openOrders.map((order) => ({ value: order.orderId, label: order.label }))}
            />
          )}
          <KitButton
            variant="destructive"
            loading={pending}
            onClick={() => {
              send(() => exchangeCancelOrderCall(cancelOrderId), setCancelOutcome);
            }}
            {...availability(walletReason, !isBytes32(cancelOrderId) && "Choose an order.")}
          >
            Cancel order
          </KitButton>
          <SendOutcome outcome={cancelOutcome} />
        </div>
      </ExplorerPanel>
      <ExplorerPanel title="Margin">
        <div className="flex flex-col gap-3">
          <TextField
            label="Deposit (native PAX)"
            inputMode="decimal"
            value={depositText}
            onChange={(event) => {
              setDepositText(event.target.value);
            }}
          />
          <KitButton
            loading={pending}
            onClick={() => {
              if (marginAccount === undefined || deposit === undefined) {
                return;
              }
              send(() => exchangeDepositMarginCall(marginAccount, deposit), setMarginOutcome);
            }}
            {...availability(walletReason, marginReason, (deposit === undefined || deposit === 0n) && "Enter an amount.")}
          >
            Deposit margin
          </KitButton>
          <TextField
            label="Withdraw asset id (bytes32)"
            value={withdrawAsset}
            spellCheck={false}
            onChange={(event) => {
              setWithdrawAsset(event.target.value.trim());
            }}
          />
          <TextField
            label="Withdraw amount (base units)"
            inputMode="numeric"
            value={withdrawText}
            onChange={(event) => {
              setWithdrawText(event.target.value);
            }}
          />
          <KitButton
            variant="secondary"
            loading={pending}
            onClick={() => {
              if (marginAccount === undefined || withdraw === undefined) {
                return;
              }
              send(() => exchangeWithdrawMarginCall(marginAccount, withdrawAsset, withdraw), setMarginOutcome);
            }}
            {...availability(
              walletReason,
              marginReason,
              (!isBytes32(withdrawAsset) || withdraw === undefined) && "Enter an asset id and an amount.",
            )}
          >
            Withdraw margin
          </KitButton>
          <SendOutcome outcome={marginOutcome} />
        </div>
      </ExplorerPanel>
      <ExplorerPanel title="Request settlement">
        <div className="flex flex-col gap-3">
          <TextField
            label="Position id (bytes32)"
            value={positionId}
            spellCheck={false}
            onChange={(event) => {
              setPositionId(event.target.value.trim());
            }}
          />
          <KitButton
            variant="secondary"
            loading={pending}
            onClick={() => {
              send(() => exchangeRequestSettlementCall(positionId), setSettleOutcome);
            }}
            {...availability(walletReason, !isBytes32(positionId) && "Enter a position id.")}
          >
            Request settlement
          </KitButton>
          <SendOutcome outcome={settleOutcome} />
        </div>
      </ExplorerPanel>
    </div>
  );
}
