"use client";

import { copyEntry } from "../../../copy/runtime.ts";
import { InlineNotice } from "../../kit/surface";
import { KitList, KitListItem } from "../../kit/display";
import { StatusPill } from "../../kit/money";
import type { JourneyProgress } from "./model.ts";

export function JourneyStages({ progress }: Readonly<{ progress: JourneyProgress }>) {
  return (
    <div className="flex flex-col gap-3">
      <KitList>
        {progress.stages.map((stage) => (
          <KitListItem
            key={stage.stageId}
            title={stage.sentence}
            trailing={<StatusPill status={stage.statusKey} />}
            trailingCaption={
              stage.receiptVerified
                ? copyEntry("verification.receipt_verified").message
                : copyEntry("verification.unverified").message
            }
          />
        ))}
      </KitList>
      {progress.refusalSentence === undefined ? null : (
        <InlineNotice tone="danger" role="alert">
          {progress.refusalSentence}
        </InlineNotice>
      )}
    </div>
  );
}
