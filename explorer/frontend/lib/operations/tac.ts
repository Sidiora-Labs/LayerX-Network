import * as tac from '@blockscout/tac-operation-lifecycle-types';

import { rightLineArrow } from 'toolkit/utils/htmlEntities';

export function getTacOperationStatus(type: tac.OperationType) {
  switch (type) {
    case tac.OperationType.TON_TAC_TON:
      return `TON ${ rightLineArrow } TAC ${ rightLineArrow } TON`;
    case tac.OperationType.TAC_TON:
      return `TAC ${ rightLineArrow } TON`;
    case tac.OperationType.TON_TAC:
      return `TON ${ rightLineArrow } TAC`;
    case tac.OperationType.ERROR:
      return 'Error';
    case tac.OperationType.ROLLBACK:
      return 'Rollback';
    case tac.OperationType.INSUFFICIENT_FEE:
      return 'Insufficient fee';
    case tac.OperationType.PENDING:
      return 'Pending';
    default:
      return null;
  }
}

export function getTacOperationStage(data: tac.OperationDetails, txHash: string) {
  const currentStep = data.status_history.filter((step) => step.transactions.some((tx) => tx.hash.toLowerCase() === txHash.toLowerCase()));
  if (currentStep.length === 0) {
    return;
  }
  return currentStep.map((step) => STATUS_LABELS[step.type]);
}

export const STATUS_SEQUENCE: Array<tac.OperationStage_StageType> = [
  tac.OperationStage_StageType.COLLECTED_IN_TAC,
  tac.OperationStage_StageType.INCLUDED_IN_TAC_CONSENSUS,
  tac.OperationStage_StageType.EXECUTED_IN_TAC,
  tac.OperationStage_StageType.COLLECTED_IN_TON,
  tac.OperationStage_StageType.INCLUDED_IN_TON_CONSENSUS,
  tac.OperationStage_StageType.EXECUTED_IN_TON,
];

export const STATUS_LABELS: Record<tac.OperationStage_StageType, string> = {
  [tac.OperationStage_StageType.COLLECTED_IN_TAC]: 'Collected in TAC',
  [tac.OperationStage_StageType.INCLUDED_IN_TAC_CONSENSUS]: 'Included in TAC consensus',
  [tac.OperationStage_StageType.EXECUTED_IN_TAC]: 'Executed in TAC',
  [tac.OperationStage_StageType.COLLECTED_IN_TON]: 'Collected in TON',
  [tac.OperationStage_StageType.INCLUDED_IN_TON_CONSENSUS]: 'Included in TON consensus',
  [tac.OperationStage_StageType.EXECUTED_IN_TON]: 'Executed in TON',
  [tac.OperationStage_StageType.UNRECOGNIZED]: 'Unknown',
};

export const sortStatusHistory = (a: tac.OperationStage, b: tac.OperationStage) => {
  const aIndex = STATUS_SEQUENCE.indexOf(a.type);
  const bIndex = STATUS_SEQUENCE.indexOf(b.type);
  return aIndex - bIndex;
};
