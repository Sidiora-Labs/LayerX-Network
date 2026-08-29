import { copyEntry } from "../../../../../copy/catalog";
import { programRecord } from "../../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerUnavailable,
  verificationLabel,
} from "../../../../explorer/components";
import {
  ExplorerFreshness,
  ExplorerTable,
  ExplorerVerificationBadge,
} from "../../../../kit";

export default async function ProgramPage({
  params,
}: Readonly<{ params: Promise<{ programId: string }> }>) {
  let program;
  try {
    program = await programRecord((await params).programId);
  } catch {
    return <ExplorerUnavailable />;
  }
  if (program === undefined) {
    return (
      <ExplorerFrame
        title={copyEntry("explorer.program.title").message}
        description={copyEntry("explorer.not_found").message}
      >
        <p className="text-sm text-foreground-secondary">
          {copyEntry("explorer.not_found.body").message}
        </p>
      </ExplorerFrame>
    );
  }
  const verified = (
    <ExplorerVerificationBadge
      label={verificationLabel("state-proven")}
    />
  );
  const policy = program.upgradePolicy.kind === "immutable"
    ? "immutable"
    : `upgradeable by ${program.upgradePolicy.authority}`;
  return (
    <ExplorerFrame
      title={copyEntry("explorer.program.title").message}
      description={program.program}
    >
      <ExplorerFreshness
        title={copyEntry("explorer.program.current").message}
        description={`Observed sequence ${program.observedSequence} at ${program.observedAt}.`}
        current
      />
      <ExplorerTable
        caption={copyEntry("explorer.program.facts").message}
        columns={[
          copyEntry("explorer.column.fact").message,
          copyEntry("explorer.column.value").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={[
          { id: "lifecycle", cells: [copyEntry("explorer.program.fact.lifecycle").message, program.lifecycle, verified] },
          { id: "policy", cells: [copyEntry("explorer.program.fact.upgrade_policy").message, policy, verified] },
          { id: "sequence", cells: [copyEntry("explorer.program.fact.observed_sequence").message, program.observedSequence, verified] },
          { id: "observed", cells: [copyEntry("explorer.program.fact.observed_at").message, program.observedAt, verified] },
          { id: "receipt", cells: [copyEntry("explorer.column.receipt").message, program.receiptDigest, verified] },
          { id: "root", cells: [copyEntry("explorer.program.fact.state_root").message, program.stateRoot, verified] },
        ]}
      />
      <ExplorerTable
        caption={copyEntry("explorer.program.versions").message}
        columns={[
          copyEntry("explorer.program.column.version").message,
          copyEntry("explorer.program.column.code_hash").message,
          copyEntry("explorer.program.column.abi").message,
          copyEntry("explorer.program.column.interface").message,
          copyEntry("explorer.program.column.source").message,
        ]}
        rows={program.versions.map((version) => ({
          id: version.version,
          cells: [
            version.version,
            version.codeHash,
            version.abiVersion,
            version.interfaceDigest ?? copyEntry("explorer.value.none").message,
            <ExplorerVerificationBadge
              key="source"
              label={copyEntry(`explorer.program.source.${version.source.status}`).message}
              unverified={version.source.status !== "verified"}
            />,
          ],
        }))}
      />
      <ExplorerTable
        caption={copyEntry("explorer.program.accounts").message}
        columns={[
          copyEntry("explorer.program.column.account").message,
          copyEntry("explorer.program.column.asset").message,
          copyEntry("explorer.program.column.balance").message,
          copyEntry("explorer.program.column.frozen").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={program.valueAccounts.map((account) => ({
          id: `${account.account}-${account.asset}`,
          cells: [account.account, account.asset, account.balance, String(account.frozen), verified],
        }))}
      />
    </ExplorerFrame>
  );
}
