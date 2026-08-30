# Code generated from platform/sdk/generators/receipt.kvx. DO NOT EDIT.

from enum import Enum

PROGRAMS_MODULE_ID: int
PROGRAM_OUTCOME_TAGS: tuple[int, int, int]

class ReceiptFailureCode(str, Enum):
    DECODE: str
    CANONICAL_ENCODING: str
    RECEIPT_SHAPE: str
    MISSING_SIGNATURE: str
    PROTOCOL_VERSION: str
    RESULT_CODE: str
    OPERATION: str
    ACTIVITY_ID: str
    GLOBAL_SEQUENCE: str
    MODULE_ID: str
    MODULE_VERSION: str
    TIMESTAMP: str
    BATCH_ID: str
    ASSET: str
    PREVIOUS_STATE_ROOT: str
    RESULTING_STATE_ROOT: str
    DEBIT_BALANCE: str
    CREDIT_BALANCE: str
    PROGRAM_OUTCOME: str
    SEQUENCER_SIGNATURE: str

REQUIRED_NONZERO_CHECKS: tuple[ReceiptFailureCode, ...]
