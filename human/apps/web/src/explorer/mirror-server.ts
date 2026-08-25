import "server-only";

import { execFile } from "node:child_process";
import { isAbsolute } from "node:path";

import type { EvidenceVerificationReport, MirrorVerificationProvenance } from "./model";
import { MirrorVerificationAdmission } from "./mirror-admission";

const MAX_EXECUTABLE_OUTPUT_BYTES = 1_048_576;
const MAX_EVIDENCE_HEX = 2_100_000;
const MAX_PROOF_HEX = 2_100_000;
const TIMEOUT_MS = 15_000;
const VERIFIER_CONCURRENCY = 4;
const VERIFIER_ADDRESS_SPACE_BYTES = 536_870_912;
const VERIFIER_FILE_BYTES = 16_777_216;
const VERIFIER_OPEN_FILES = 64;
const VERIFIER_PROCESSES = 32;
const MAX_U64 = 18_446_744_073_709_551_615n;

export class MirrorUnavailableError extends Error { public constructor(){super("mirror unavailable");this.name="MirrorUnavailableError";} }
export class MirrorDivergentError extends Error { public constructor(){super("mirror divergent");this.name="MirrorDivergentError";} }
export class MirrorRefusedError extends Error { public constructor(){super("mirror refused");this.name="MirrorRefusedError";} }

const admission = new MirrorVerificationAdmission(VERIFIER_CONCURRENCY);

type Json = Readonly<Record<string, unknown>>;
function record(value:unknown):Json { if(typeof value!=="object"||value===null||Array.isArray(value))throw new MirrorRefusedError();return value as Json; }
function decimal(value:unknown):string { if(typeof value!=="string"||!/^[1-9]\d*$/u.test(value)||BigInt(value)>MAX_U64)throw new MirrorRefusedError();return value; }
function hex(value:unknown,maximum:number):string { if(typeof value!=="string"||value.length===0||value.length>maximum||value.length%2!==0||!/^[0-9a-f]+$/u.test(value))throw new MirrorRefusedError();return value; }
function digest(value:unknown):string { const result=hex(value,64);if(result.length!==64)throw new MirrorRefusedError();return result; }
function integer(value:unknown,maximum:number):number { if(typeof value!=="number"||!Number.isSafeInteger(value)||value<0||value>maximum)throw new MirrorRefusedError();return value; }

function executableConfiguration():Readonly<{runner:string;executable:string;configuration:string}>{
  const runner=process.env.LAYERX_MIRROR_VERIFIER_PRLIMIT_EXECUTABLE;const executable=process.env.LAYERX_MIRROR_VERIFIER_EXECUTABLE;const configuration=process.env.LAYERX_MIRROR_VERIFIER_CONFIG;
  if(runner===undefined||executable===undefined||configuration===undefined||!isAbsolute(runner)||!isAbsolute(executable)||!isAbsolute(configuration))throw new MirrorUnavailableError();
  return {runner,executable,configuration};
}

function evidenceRequest(kind:"receipt"|"state-inclusion", encoded:string):Json {
  let parsed:unknown;try{parsed=JSON.parse(encoded);}catch{throw new MirrorRefusedError();}const item=record(parsed);const batch=decimal(item.batch_number);const canonical=hex(item.canonical_hex,MAX_EVIDENCE_HEX);const policyItem=record(item.policy);const policyKind=policyItem.kind;
  if(policyKind!=="exact"&&policyKind!=="ordered-preference"&&policyKind!=="agreement")throw new MirrorRefusedError();
  const rawCandidates=policyKind==="exact"?[policyItem.candidate]:policyItem.candidates;if(!Array.isArray(rawCandidates)||rawCandidates.length===0||rawCandidates.length>8)throw new MirrorRefusedError();const seen=new Set<number>();const candidates=rawCandidates.map((raw)=>{const value=record(raw);const source=integer(value.source,7);if(seen.has(source))throw new MirrorRefusedError();seen.add(source);return {source,commitment_hex:digest(value.commitment_hex)};});
  let policy:Json;if(policyKind==="exact"){if(candidates.length!==1)throw new MirrorRefusedError();policy={kind:"exact",candidate:candidates[0]};}else if(policyKind==="agreement"){const minimum=integer(policyItem.minimum,candidates.length);if(minimum===0)throw new MirrorRefusedError();policy={kind:policyKind,candidates,minimum};}else{policy={kind:policyKind,candidates};}
  const evidence=kind==="receipt"?{kind:"receipt",canonical_hex:canonical}:{kind:"state",canonical_hex:canonical,proof_hex:hex(item.proof_hex,MAX_PROOF_HEX)};
  return {batch_number:batch,evidence,policy};
}

async function execute(request:Json,signal?:AbortSignal):Promise<Json>{
  const {runner,executable,configuration}=executableConfiguration();const input=JSON.stringify(request);const arguments_=[`--as=${VERIFIER_ADDRESS_SPACE_BYTES}`,`--fsize=${VERIFIER_FILE_BYTES}`,`--nofile=${VERIFIER_OPEN_FILES}`,`--nproc=${VERIFIER_PROCESSES}`,"--core=0","--",executable,configuration];return new Promise((resolve,reject)=>{const child=execFile(runner,arguments_,{encoding:"utf8",maxBuffer:MAX_EXECUTABLE_OUTPUT_BYTES,timeout:TIMEOUT_MS,killSignal:"SIGKILL",windowsHide:true,signal},(error,stdout)=>{if(error!==null){reject(new MirrorUnavailableError());return;}try{resolve(record(JSON.parse(stdout)));}catch{reject(new MirrorUnavailableError());}});child.stdin?.end(input);});
}

function level(value:unknown):EvidenceVerificationReport["achievedLevel"]{if(typeof value!=="string")throw new MirrorRefusedError();const normalized=value.replaceAll("_","").replaceAll("-","").toLowerCase();switch(normalized){case"sequencersigned":return"sequencer-signed";case"batchincluded":return"batch-included";case"stateproven":return"state-proven";case"checkpointfinalised":return"checkpoint-finalised";case"settlementanchored":return"settlement-anchored";default:throw new MirrorRefusedError();}}
function text(value:unknown):string{if(typeof value!=="string"||value.length===0||value.length>2048)throw new MirrorRefusedError();return value;}

export async function verifyEvidenceFromMirrors(input:Readonly<{kind:"receipt"|"state-inclusion";evidence:string}>,signal?:AbortSignal):Promise<EvidenceVerificationReport>{
  const request=evidenceRequest(input.kind,input.evidence);return admission.run(async()=>{const response=await execute(request,signal);if(response.ok!==true){const code=response.error;if(code==="divergent")throw new MirrorDivergentError();if(code==="source-unavailable"||code==="unavailable"||code==="rate-limited"||code==="rpc-divergent")throw new MirrorUnavailableError();throw new MirrorRefusedError();}
  const value=record(response.verification);const latest=value.latestBatch===null||value.latestBatch===undefined?undefined:decimal(value.latestBatch);const lag=text(value.batchLag);const known=/^Known\((\d+)\)$/u.exec(lag);const provenance=text(value.provenance).toLowerCase();if(provenance!=="canonical"&&provenance!=="reorged")throw new MirrorRefusedError();
  const mirror:MirrorVerificationProvenance=Object.freeze({sourceId:text(value.sourceId),target:text(value.target),canonicalPosition:text(value.canonicalPosition),provenance,latestBatch:latest,batchLag:known===null?Object.freeze({kind:"unknown"}):Object.freeze({kind:"known",batches:known[1]??"0"}),failoverCount:String(integer(value.failoverCount,8)),agreeingSources:String(integer(value.agreeingSources,8)),checkpointLevel:"unavailable",degraded:provenance==="reorged"||known===null});
  return Object.freeze({kind:input.kind,achievedLevel:level(value.level),...(input.kind==="receipt"?{receiptDigest:digest(value.evidenceDigest)}:{}),headerDigest:digest(value.headerDigest),mirror});});
}
