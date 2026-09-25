"use client";

import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { launchpadBuyCall, launchpadCreateMarketCall, launchpadSellCall } from "../../api/sdk";
import { sendWalletPrecompileCall, type PrecompileSendOutcome } from "../../api/wallet";
import { KitButton } from "../../kit/control";
import { ExplorerPanel } from "../../kit/explorer";
import { TextField } from "../../kit/field";
import { LabelValue } from "../../kit/money";
import { SegmentedControl } from "../../kit/collection";
import { InlineNotice } from "../../kit/surface";
import { availability } from "../_markets/availability";
import { LAUNCHPAD_DECIMALS, formatUnits, parseUnits } from "../_markets/format";
import { ChoiceField } from "../_markets/select";
import { SendOutcome } from "../_markets/send-outcome";
import { quoteLaunchpadSwap, type LaunchpadQuoteAnswer } from "./actions";

const DEADLINE_SECONDS = 600n;
const BASIS_POINTS = 10_000n;

type Side = "buy" | "sell";

interface PreviewedQuote {
  readonly token: string;
  readonly side: Side;
  readonly amountIn: bigint;
  readonly slippageBps: bigint;
  readonly answer: LaunchpadQuoteAnswer;
}

export function LaunchpadActions({
  account,
  quoteDenom,
  markets,
  feeStrategies,
}: Readonly<{
  account: string | undefined;
  quoteDenom: string;
  markets: readonly Readonly<{ token: string; label: string; paused: boolean }>[];
  feeStrategies: readonly string[];
}>) {
  const router = useRouter();
  const [pending, startTransition] = useTransition();
  const [token, setToken] = useState(markets[0]?.token ?? "");
  const [side, setSide] = useState<Side>("buy");
  const [amountText, setAmountText] = useState("");
  const [slippageText, setSlippageText] = useState("100");
  const [preview, setPreview] = useState<PreviewedQuote | undefined>(undefined);
  const [tradeOutcome, setTradeOutcome] = useState<PrecompileSendOutcome | undefined>(undefined);
  const [name, setName] = useState("");
  const [symbol, setSymbol] = useState("");
  const [strategy, setStrategy] = useState("0");
  const [createOutcome, setCreateOutcome] = useState<PrecompileSendOutcome | undefined>(undefined);

  const amountIn = parseUnits(amountText, LAUNCHPAD_DECIMALS);
  const slippage = /^[0-9]{1,4}$/u.test(slippageText) ? BigInt(slippageText) : undefined;
  const validTrade = token !== "" && amountIn !== undefined && amountIn > 0n && slippage !== undefined && slippage <= BASIS_POINTS;
  const currentPreview =
    preview !== undefined &&
    preview.token === token &&
    preview.side === side &&
    preview.amountIn === amountIn &&
    preview.slippageBps === slippage
      ? preview
      : undefined;
  const quote = currentPreview?.answer.ok === true ? currentPreview.answer : undefined;
  const minOut = quote === undefined || slippage === undefined ? undefined : (BigInt(quote.amountOut) * (BASIS_POINTS - slippage)) / BASIS_POINTS;
  const payDenom = side === "buy" ? quoteDenom : "token";
  const receiveDenom = side === "buy" ? "token" : quoteDenom;

  const previewQuote = () => {
    if (!validTrade) {
      return;
    }
    const request = { token, side, amountIn, slippageBps: slippage };
    startTransition(async () => {
      const answer = await quoteLaunchpadSwap(token, side, amountIn.toString());
      setPreview({ ...request, answer });
    });
  };

  const trade = () => {
    if (account === undefined || quote === undefined || minOut === undefined || amountIn === undefined) {
      return;
    }
    const order = {
      token,
      amountIn,
      minOut,
      recipient: account,
      deadline: BigInt(Math.floor(Date.now() / 1000)) + DEADLINE_SECONDS,
    };
    startTransition(async () => {
      const outcome = await sendWalletPrecompileCall(account, () =>
        side === "buy" ? launchpadBuyCall(order) : launchpadSellCall(order),
      );
      setTradeOutcome(outcome);
      if (outcome.outcome === "sent") {
        setPreview(undefined);
        router.refresh();
      }
    });
  };

  const create = () => {
    if (account === undefined) {
      return;
    }
    startTransition(async () => {
      const outcome = await sendWalletPrecompileCall(account, () =>
        launchpadCreateMarketCall(name.trim(), symbol.trim(), Number(strategy)),
      );
      setCreateOutcome(outcome);
      if (outcome.outcome === "sent") {
        router.refresh();
      }
    });
  };

  const walletReason = account === undefined ? "Connect a wallet first." : undefined;

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <ExplorerPanel title="Buy or sell">
        {markets.length === 0 ? (
          <InlineNotice>There is no market to trade yet.</InlineNotice>
        ) : (
          <div className="flex flex-col gap-3">
            <ChoiceField
              label="Market"
              value={token}
              onChange={setToken}
              options={markets.map((market) => ({
                value: market.token,
                label: market.paused ? `${market.label} — paused` : market.label,
                disabled: market.paused,
              }))}
            />
            <SegmentedControl
              aria-label="Side"
              value={side}
              onValueChange={(value) => {
                setSide(value === "sell" ? "sell" : "buy");
              }}
              options={[
                { value: "buy", label: "Buy" },
                { value: "sell", label: "Sell" },
              ]}
            />
            <TextField
              label={`You pay (${payDenom})`}
              inputMode="decimal"
              value={amountText}
              onChange={(event) => {
                setAmountText(event.target.value);
              }}
              errorMessage={amountText !== "" && amountIn === undefined ? "Enter an amount with at most six decimals." : undefined}
            />
            <TextField
              label="Slippage tolerance (basis points)"
              inputMode="numeric"
              value={slippageText}
              onChange={(event) => {
                setSlippageText(event.target.value);
              }}
              errorMessage={slippage === undefined || slippage > BASIS_POINTS ? "Enter 0 to 10000." : undefined}
            />
            <KitButton
              variant="secondary"
              loading={pending}
              onClick={previewQuote}
              {...availability(!validTrade && "Choose a market and an amount.")}
            >
              Preview quote
            </KitButton>
            {currentPreview?.answer.ok === false ? (
              <InlineNotice tone="warning">{currentPreview.answer.detail}</InlineNotice>
            ) : null}
            {quote === undefined || minOut === undefined ? null : (
              <div className="flex flex-wrap gap-6">
                <LabelValue label={`You receive (${receiveDenom})`} value={formatUnits(BigInt(quote.amountOut), LAUNCHPAD_DECIMALS)} />
                <LabelValue label="Fee" value={`${formatUnits(BigInt(quote.feeAmount), LAUNCHPAD_DECIMALS)} (${quote.feeBps} bps)`} />
                <LabelValue label="Minimum received" value={formatUnits(minOut, LAUNCHPAD_DECIMALS)} />
              </div>
            )}
            <KitButton
              loading={pending}
              onClick={trade}
              {...availability(walletReason, quote === undefined && "Preview the quote for these exact inputs first.")}
            >
              {side === "buy" ? "Buy" : "Sell"}
            </KitButton>
            <SendOutcome outcome={tradeOutcome} />
          </div>
        )}
      </ExplorerPanel>
      <ExplorerPanel title="Create a market">
        <div className="flex flex-col gap-3">
          <TextField
            label="Name"
            value={name}
            maxLength={64}
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
          <TextField
            label="Symbol"
            value={symbol}
            maxLength={16}
            onChange={(event) => {
              setSymbol(event.target.value.toUpperCase());
            }}
          />
          <ChoiceField
            label="Fee strategy"
            value={strategy}
            onChange={setStrategy}
            options={feeStrategies.map((label, index) => ({ value: String(index), label }))}
          />
          <KitButton
            loading={pending}
            onClick={create}
            {...availability(walletReason, (name.trim() === "" || symbol.trim() === "") && "Name and symbol are required.")}
          >
            Create market
          </KitButton>
          <SendOutcome outcome={createOutcome} />
        </div>
      </ExplorerPanel>
    </div>
  );
}
