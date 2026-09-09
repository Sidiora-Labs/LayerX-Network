import { spawn } from "node:child_process";
import { isAbsolute } from "node:path";
import { createHash } from "node:crypto";
import { JsonRpcError, type Commitment } from "./rpc.js";

export interface WalletExecutionOptions {
  readonly actor: string;
  readonly idempotencyKey: string;
  readonly feeLimit: bigint;
  readonly notBefore: bigint;
  readonly notAfter: bigint;
  readonly commitment: Commitment;
  readonly timeoutMs: number;
}
export interface WalletRpcConfiguration {
  readonly endpoint: string;
  readonly protocol_version: string;
  readonly network_id: string;
  readonly native_asset: string;
  readonly sequencer_id: string;
  readonly sequencer_key: string;
  readonly first_batch: string;
  readonly last_batch: string;
  readonly checkpoint_context_digest?: string;
  readonly signer_socket?: string;
  readonly signer_public_key?: string;
  readonly key_id?: string;
  readonly key_secret?: string;
}
export interface NativeTokenRegistration {
  readonly salt: string;
  readonly symbol: string;
  readonly name: string;
  readonly decimals: number;
  readonly supplyCap: bigint;
}
export interface VerifiedWalletReceipt {
  readonly receipt: string;
  readonly activityId: string;
  readonly resultCode: number;
  readonly state: "executed" | "refused";
  readonly commitment: Commitment;
}

export class WalletPendingError extends Error { public constructor(public readonly activityId:string){super("Activity remains pending");} }

export class WalletRpc {
  readonly #executable: string;
  readonly #configuration: WalletRpcConfiguration;
  public constructor(executable: string, configuration: WalletRpcConfiguration) {
    if (!isAbsolute(executable)) throw new Error("Wallet executable must be an absolute trusted path");
    this.#executable = executable;
    this.#configuration = Object.freeze({...configuration});
  }
  public toJSON(): string { return "[WalletRpc REDACTED]"; }
  public readonly wallet = {
    accounts: (did: string): Promise<Record<string, unknown>> => this.call({action:"accounts", did}),
    balance: (did: string, asset: string): Promise<Record<string, unknown>> => this.call({action:"balance", did, asset}),
    send: (canonicalSendPayload: Uint8Array, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(5, canonicalSendPayload, options),
    openAccount: (asset: string, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(4, Buffer.concat([integer(1n,2), id(asset)]), options),
  };
  public readonly token = {
    create: (registration: NativeTokenRegistration, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(1, encodeNativeRegistration(registration, options.actor), options),
    mint: (asset: string, toAccount: string, amount: bigint, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(10, amountPayload(asset,toAccount,amount), options),
    burn: (asset: string, fromAccount: string, amount: bigint, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(11, amountPayload(asset,fromAccount,amount), options),
    transfer: (canonicalSendPayload: Uint8Array, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.wallet.send(canonicalSendPayload,options),
  };
  public readonly grant = {
    issue: (canonicalGrant: Uint8Array, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(7,canonicalGrant,options),
    revoke: (grant: string, sequence: bigint, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(8,Buffer.concat([integer(1n,2),id(grant),integer(sequence,8)]),options),
    draw: (request: {from:string;to:string;asset:string;amount:bigint;grant:string;contextHash:string}, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> => this.submit(6,Buffer.concat([integer(0x5201n,2),integer(8n,2),id(request.from),id(request.to),id(request.asset),integer(request.amount,16),id(request.grant),integer(0n,8),id(options.idempotencyKey),id(request.contextHash)]),options),
  };
  public async waitFor(activity: string, commitment: Commitment, timeoutMs = 30000): Promise<VerifiedWalletReceipt> {
    return receipt(await this.call({action:"wait",activity_id:activity,commitment,timeout_ms:checkedTimeout(timeoutMs).toString()},timeoutMs),commitment);
  }
  private async submit(ordinal: number, payload: Uint8Array, options: WalletExecutionOptions): Promise<VerifiedWalletReceipt> {
    if (payload.length === 0 || payload.length > 524288) throw new Error("Invalid payment payload length");
    return receipt(await this.call({action:"submit",ordinal:String(ordinal),payload:Buffer.from(payload).toString("hex"),actor:options.actor,idempotency_key:options.idempotencyKey,fee_limit:options.feeLimit.toString(),not_before:options.notBefore.toString(),not_after:options.notAfter.toString(),commitment:options.commitment,timeout_ms:checkedTimeout(options.timeoutMs).toString()},options.timeoutMs),options.commitment);
  }
  private call(request: Record<string,unknown>, waitMs = 30000): Promise<Record<string,unknown>> {
    const input = Buffer.from(JSON.stringify({...request,configuration:this.#configuration}));
    if (input.length > 2097152) throw new Error("Wallet request exceeds bound");
    return new Promise((resolve,reject) => {
      const child = spawn(this.#executable,[],{stdio:["pipe","pipe","ignore"],windowsHide:true});
      const chunks: Buffer[] = []; let size=0;
      const timer = setTimeout(()=>child.kill("SIGKILL"),checkedTimeout(waitMs)+180000);
      child.on("error",reject);
      child.stdin.on("error",reject);
      child.stdout.on("data",(chunk:Buffer)=>{size+=chunk.length;if(size>9*1048576){child.kill("SIGKILL");reject(new Error("Wallet response exceeds bound"));return;}chunks.push(chunk);});
      child.on("close",(code)=>{
        clearTimeout(timer);
        try {
          if(code!==0) throw new Error("Wallet execution unavailable; submission outcome may be unknown");
          const output:unknown=JSON.parse(new TextDecoder("utf-8",{fatal:true}).decode(Buffer.concat(chunks)));
          if(!object(output)) throw new Error("Invalid wallet result");
          if(output.ok!==true){
            if(object(output.error)&&Number.isSafeInteger(output.error.code)&&typeof output.error.message==="string") throw new JsonRpcError(output.error.code as number,output.error.message,output.error.data);
            if(object(output.error)&&output.error.kind==="pending"&&typeof output.error.activity_id==="string") throw new WalletPendingError(output.error.activity_id);
            throw new Error("Wallet request refused or unavailable");
          }
          if(!object(output.result)) throw new Error("Invalid wallet result");
          resolve(output.result);
        } catch(error) { reject(error); }
      });
      child.stdin.end(input);
    });
  }
}
function object(v:unknown):v is Record<string,unknown>{return typeof v==="object"&&v!==null&&!Array.isArray(v);}
function checkedTimeout(n:number):number{if(!Number.isSafeInteger(n)||n<0||n>300000)throw new Error("Invalid wallet timeout");return n;}
function receipt(v:Record<string,unknown>,commitment:Commitment):VerifiedWalletReceipt{
  if(typeof v.receipt!=="string"||!/^([0-9a-f]{2})+$/u.test(v.receipt)||typeof v.activity_id!=="string"||!/^[0-9a-f]{64}$/u.test(v.activity_id)||!Number.isSafeInteger(v.result_code)||v.commitment!==commitment||v.state!==(v.result_code===0?"executed":"refused"))throw new Error("Invalid verified wallet response");
  return Object.freeze({receipt:v.receipt,activityId:v.activity_id,resultCode:v.result_code as number,state:v.state as "executed"|"refused",commitment});
}
function id(s:string):Buffer{if(!/^[0-9a-f]{64}$/u.test(s))throw new Error("Invalid payment identifier");return Buffer.from(s,"hex");}
function integer(n:bigint,size:number):Buffer{if(n<0n||n>=(1n<<BigInt(size*8)))throw new Error("Payment integer out of range");const b=Buffer.alloc(size);for(let i=size-1;i>=0;i--){b[i]=Number(n&255n);n>>=8n;}return b;}
function amountPayload(asset:string,account:string,amount:bigint):Buffer{if(amount===0n)throw new Error("Zero payment amount");return Buffer.concat([integer(1n,2),id(asset),id(account),integer(amount,16)]);}
export function encodeNativeRegistration(registration:NativeTokenRegistration,actor:string):Uint8Array {
  const did=Buffer.from(actor),symbol=Buffer.from(registration.symbol),name=Buffer.from(registration.name);
  if(did.length===0||did.length>255||symbol.length===0||symbol.length>16||symbol.some(b=>b>127)||name.length===0||name.length>32||!Number.isSafeInteger(registration.decimals)||registration.decimals<0||registration.decimals>38)throw new Error("Invalid token registration");
  const issuer=createHash("sha256").update("LXP/v1/did-id\0").update(integer(BigInt(did.length),2)).update(did).digest();
  const salt=id(registration.salt),asset=createHash("sha256").update("LX:ASSET:v1").update(issuer).update(salt).digest();
  return Buffer.concat([integer(1n,2),asset,salt,Buffer.from([symbol.length]),symbol,Buffer.from([name.length]),name,Buffer.from([registration.decimals]),integer(registration.supplyCap,16),Buffer.from([1,0])]);
}
