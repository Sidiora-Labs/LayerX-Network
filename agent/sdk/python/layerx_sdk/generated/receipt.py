# Code generated from platform/sdk/generators/receipt.kvx. DO NOT EDIT.

from enum import Enum

PROGRAMS_MODULE_ID = 9
PROGRAM_OUTCOME_TAGS = (1347569457, 1347569458, 1347569459)

class ReceiptFailureCode(str, Enum):
    DECODE = "decode"
    CANONICAL_ENCODING = "canonical-encoding"
    RECEIPT_SHAPE = "receipt-shape"
    MISSING_SIGNATURE = "missing-signature"
    PROTOCOL_VERSION = "protocol-version"
    RESULT_CODE = "result-code"
    OPERATION = "operation"
    ACTIVITY_ID = "activity-id"
    GLOBAL_SEQUENCE = "global-sequence"
    MODULE_ID = "module-id"
    MODULE_VERSION = "module-version"
    TIMESTAMP = "timestamp"
    BATCH_ID = "batch-id"
    ASSET = "asset"
    PREVIOUS_STATE_ROOT = "previous-state-root"
    RESULTING_STATE_ROOT = "resulting-state-root"
    DEBIT_BALANCE = "debit-balance"
    CREDIT_BALANCE = "credit-balance"
    PROGRAM_OUTCOME = "program-outcome"
    SEQUENCER_SIGNATURE = "sequencer-signature"

REQUIRED_NONZERO_CHECKS = (
    ReceiptFailureCode.GLOBAL_SEQUENCE,
    ReceiptFailureCode.MODULE_ID,
    ReceiptFailureCode.MODULE_VERSION,
    ReceiptFailureCode.TIMESTAMP,
    ReceiptFailureCode.ACTIVITY_ID,
    ReceiptFailureCode.RESULTING_STATE_ROOT,
)
