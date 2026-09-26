import { chooseWalletFee, sidioraAmount, walletFeePreference, type WalletFeePreference } from "../../api/wallet.ts";
import type {
  BindingStatement,
  EvidenceRef,
  HumanApiClient,
  Journey,
  NotificationSummary,
  WalletBinding,
} from "../../api/index.ts";
import {
  browserPasskeyAuthenticator,
  performStepUp,
  type PasskeyAuthenticator,
} from "../../journeys/approvals/index.ts";
import { verificationAtLeast } from "../../journeys/custody/evidence.ts";
import { newIdempotencyKey } from "../../journeys/custody/model.ts";
import type {
  BindingWalletBridge,
  WalletBridgeFailure,
} from "./bridge.ts";

export const WALLET_BINDING_DOMAIN = "layerx-wallet-binding-v1\n";
const EVM_ADDRESS = /^0x[0-9a-fA-F]{40}$/u;

export interface ActiveWalletBinding {
  readonly address: string;
  readonly boundAt: string;
  readonly evidence: EvidenceRef;
}

export type WalletBindingPhase =
  | "ready"
  | "waiting"
  | "cancelled"
  | "rejected"
  | "unavailable"
  | "failed"
  | "submitted"
  | "active";

export interface WalletBindingSnapshot {
  readonly status?: WalletBinding;
  readonly active?: ActiveWalletBinding;
  readonly candidate?: string;
  readonly journey?: Journey;
  readonly phase: WalletBindingPhase;
}

export function receiptBackedBinding(binding: WalletBinding): ActiveWalletBinding | undefined {
  if (
    (binding.state !== "bound" && binding.state !== "rebinding")
    || binding.address === undefined
    || binding.bound_at === undefined
    || binding.evidence?.class !== "layerx-receipt"
    || !verificationAtLeast(binding.evidence.verification, "receipt-verified")
    || !EVM_ADDRESS.test(binding.address)
  ) {
    return undefined;
  }
  return Object.freeze({
    address: binding.address,
    boundAt: binding.bound_at,
    evidence: binding.evidence,
  });
}

export function validBindingStatement(
  statement: BindingStatement,
  selectedAddress: string,
  now = Date.now(),
): boolean {
  const expiry = Date.parse(statement.expires_at);
  return EVM_ADDRESS.test(statement.address)
    && statement.address.toLowerCase() === selectedAddress.toLowerCase()
    && statement.statement.startsWith(WALLET_BINDING_DOMAIN)
    && statement.statement.includes(`\naddress: ${statement.address}\n`)
    && Number.isFinite(expiry)
    && expiry > now;
}

export function newestWalletSecurityNotification(
  notifications: readonly NotificationSummary[],
): NotificationSummary | undefined {
  return notifications
    .filter((notification) =>
      notification.class === "security-wallet-rebinding"
      && notification.deep_link === "/app/settings/wallet"
      && notification.action_copy_key !== undefined)
    .sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at))[0];
}

export class WalletBindingController {
  readonly #client: HumanApiClient;
  readonly #bridge: BindingWalletBridge;
  readonly #authenticator: PasskeyAuthenticator;
  #snapshot: WalletBindingSnapshot = Object.freeze({ phase: "ready" });
  #attempt = 0;

  constructor(options: Readonly<{
    client: HumanApiClient;
    bridge: BindingWalletBridge;
    authenticator?: PasskeyAuthenticator;
  }>) {
    this.#client = options.client;
    this.#bridge = options.bridge;
    this.#authenticator = options.authenticator ?? browserPasskeyAuthenticator();
  }

  get snapshot(): WalletBindingSnapshot {
    return this.#snapshot;
  }

  async load(): Promise<WalletBindingSnapshot> {
    const status = await this.#client.bindingStatus();
    const active = receiptBackedBinding(status);
    this.#snapshot = Object.freeze({
      status,
      ...(active === undefined ? {} : { active }),
      phase: active === undefined ? "ready" : "active",
    });
    return this.#snapshot;
  }

  cancel(): void {
    if (this.#snapshot.phase !== "waiting") {
      return;
    }
    this.#attempt += 1;
    this.#snapshot = Object.freeze({ ...this.#snapshot, phase: "cancelled" });
  }

  async rebind(): Promise<WalletBindingSnapshot> {
    if (this.#snapshot.active === undefined || this.#snapshot.phase === "waiting") {
      return this.#snapshot;
    }
    this.#attempt += 1;
    const attempt = this.#attempt;
    const previous = this.#snapshot.active;
    this.#snapshot = Object.freeze({ ...this.#snapshot, phase: "waiting" });
    const account = await this.#bridge.account();
    if (account.outcome !== "approved") {
      return this.#finishWalletFailure(attempt, account.outcome);
    }
    const action = await this.#client.bindingRebindAction({ address: account.address });
    if (!validBindingStatement(action.binding, account.address)) {
      this.#snapshot = Object.freeze({ ...this.#snapshot, phase: "failed" });
      return this.#snapshot;
    }
    const stepUp = await performStepUp(this.#client, action.confirms, this.#authenticator);
    if (this.#attempt !== attempt) {
      return this.#snapshot;
    }
    const signed = await this.#bridge.sign(action.binding.address, action.binding.statement);
    if (signed.outcome !== "approved") {
      return this.#finishWalletFailure(attempt, signed.outcome);
    }
    if (this.#attempt !== attempt) {
      return this.#snapshot;
    }
    const journey = await this.#client.bindingRebind({
      address: action.binding.address,
      statement: action.binding.statement,
      signature: signed.signature,
      step_up: stepUp,
    }, newIdempotencyKey());
    this.#snapshot = Object.freeze({
      ...(this.#snapshot.status === undefined ? {} : { status: this.#snapshot.status }),
      active: previous,
      candidate: action.binding.address,
      journey,
      phase: "submitted",
    });
    return this.#snapshot;
  }

  async refresh(): Promise<WalletBindingSnapshot> {
    const currentJourney = this.#snapshot.journey;
    const [status, journey] = await Promise.all([
      this.#client.bindingStatus(),
      currentJourney === undefined
        ? Promise.resolve(undefined)
        : this.#client.journeyGet(currentJourney.journey_id),
    ]);
    const verified = receiptBackedBinding(status);
    const candidate = this.#snapshot.candidate;
    const activated = verified !== undefined
      && candidate !== undefined
      && verified.address.toLowerCase() === candidate.toLowerCase();
    this.#snapshot = Object.freeze({
      status,
      ...(activated
        ? { active: verified }
        : this.#snapshot.active === undefined ? {} : { active: this.#snapshot.active }),
      ...(candidate === undefined ? {} : { candidate }),
      ...(journey === undefined ? {} : { journey }),
      phase: activated ? "active" : this.#snapshot.phase,
    });
    return this.#snapshot;
  }

  #finishWalletFailure(attempt: number, failure: WalletBridgeFailure): WalletBindingSnapshot {
    if (this.#attempt === attempt) {
      this.#snapshot = Object.freeze({ ...this.#snapshot, phase: failure });
    }
    return this.#snapshot;
  }
}

export const wallet_binding = Object.freeze({
  receiptBackedBinding,
  validBindingStatement,
  newestWalletSecurityNotification,
  WalletBindingController,
});

export function human_web_wallet_binding() {
  return wallet_binding;
}


export interface WalletFeeSnapshot {
  readonly currency: WalletFeePreference["currency"];
  readonly maximum: string;
  readonly valid: boolean;
}

export class WalletFeeController {
  #snapshot: WalletFeeSnapshot;

  constructor() {
    const preference = walletFeePreference();
    this.#snapshot = Object.freeze({
      currency: preference.currency,
      maximum: preference.maximum === undefined ? "" : sidioraAmount(preference.maximum),
      valid: preference.currency === "paxeer" || preference.maximum !== undefined,
    });
  }

  get snapshot(): WalletFeeSnapshot {
    return this.#snapshot;
  }

  choose(currency: WalletFeePreference["currency"], maximum: string): WalletFeeSnapshot {
    const valid = chooseWalletFee(currency, maximum);
    this.#snapshot = Object.freeze({ currency, maximum, valid });
    return this.#snapshot;
  }
}
