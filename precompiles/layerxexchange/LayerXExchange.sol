// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant LAYERX_EXCHANGE_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001015;

ILayerXExchange constant LAYERX_EXCHANGE_CONTRACT = ILayerXExchange(LAYERX_EXCHANGE_PRECOMPILE_ADDRESS);

/// LayerX exchange intents. Every write records a pending intent keyed by its
/// intentId and emits it for the LayerX intent router; no order is matched on
/// Paxeer. Margin moves only into the layerxcustody module account: a margin
/// deposit is a layerxcustody deposit, and a margin withdrawal is paid later by
/// a proof-carrying layerxCustody withdrawal of the resulting LayerX receipt.
///
/// Views read finalized LayerX state only: the caller supplies a native state
/// witness and the batch it is for, and the precompile proves it against the
/// batch's finalized state root from the anchor. Nothing is read over the
/// network.
///
/// Amounts, prices and quantities are LayerX u128 values. side: 1 buy, 2 sell.
/// timeInForce: 0 good-till-cancelled, 1 immediate-or-cancel, 2 fill-or-kill,
/// 3 post-only.
///
///   intentId = sha256(abi.encode("LXP/Paxeer/exchange-intent/v1", chainid, this, owner, uint8 kind, uint64 nonce))
/// kind: 1 deposit, 2 withdraw, 3 place, 4 cancel, 5 settle. nonce is the
/// owner's 1-based intent counter shared by every kind.
///
/// Gas = 3000 + 16 * len(calldata after the selector) + 4000 * signatures
///     + 100 * proofNodes + 5000 * writes
/// signatures: 0 for every method. proofNodes: len(witness) / 32 for the state
/// views, 0 otherwise. writes: 11 for depositMargin and depositMarginToken, 3
/// for the other writes, 0 for views.
interface ILayerXExchange {
    /// kind: 1 deposit, 2 withdraw, 3 place, 4 cancel, 5 settle. status: 0 none, 1 pending.
    struct Intent {
        bytes32 intentId;
        uint8 kind;
        uint8 status;
        address owner;
        uint64 nonce;
        uint64 height;
        bytes32 account;
        bytes32 assetId;
        string denom;
        uint256 amount;
        bytes32 depositId;
        bytes32 marketId;
        uint8 side;
        uint256 price;
        uint256 quantity;
        uint8 timeInForce;
        bytes32 orderId;
        bytes32 positionId;
    }

    /// A LayerX perps state entry proven under a finalized state root. value
    /// is the canonical LayerX encoding.
    struct StateRecord {
        uint64 batchNumber;
        bytes32 stateRoot;
        bytes key;
        bytes value;
    }

    /// A LayerX account balance proven under a finalized state root.
    struct Margin {
        uint64 batchNumber;
        bytes32 stateRoot;
        bytes32 account;
        bytes32 assetId;
        uint256 balance;
        bool frozen;
    }

    event MarginDeposited(
        bytes32 indexed intentId,
        bytes32 indexed account,
        address indexed owner,
        bytes32 assetId,
        uint256 amount,
        bytes32 depositId,
        uint64 nonce
    );
    event MarginWithdrawalRequested(
        bytes32 indexed intentId,
        bytes32 indexed account,
        address indexed owner,
        bytes32 assetId,
        uint256 amount,
        uint64 nonce
    );
    event OrderPlaced(
        bytes32 indexed intentId,
        bytes32 indexed marketId,
        address indexed owner,
        uint8 side,
        uint256 price,
        uint256 quantity,
        uint8 timeInForce,
        uint64 nonce
    );
    event OrderCancelRequested(bytes32 indexed intentId, bytes32 indexed orderId, address indexed owner, uint64 nonce);
    event SettlementRequested(
        bytes32 indexed intentId, bytes32 indexed positionId, address indexed owner, uint64 nonce
    );

    /// Custody msg.value of the native coin as margin of a LayerX account.
    function depositMargin(bytes32 account) external payable returns (bytes32 intentId, bytes32 depositId);

    /// Custody a bank denom addressed by its registered ERC20 pointer as margin.
    function depositMarginToken(address pointer, uint256 amount, bytes32 account)
        external
        returns (bytes32 intentId, bytes32 depositId);

    /// Ask LayerX to release margin of the account to its withdrawable balance.
    function withdrawMargin(bytes32 account, bytes32 assetId, uint256 amount) external returns (bytes32 intentId);

    function placeOrder(bytes32 market, uint8 side, uint256 price, uint256 qty, uint8 tif)
        external
        returns (bytes32 intentId);

    function cancelOrder(bytes32 orderId) external returns (bytes32 intentId);

    function requestSettlement(bytes32 positionId) external returns (bytes32 intentId);

    function getIntent(bytes32 intentId) external view returns (Intent memory intent);

    function intentNonce(address owner) external view returns (uint64 nonce);

    function getMarket(bytes32 marketId, uint64 batchNumber, bytes calldata witness)
        external
        view
        returns (StateRecord memory record);

    function getOrder(bytes32 marketId, bytes32 orderId, uint64 batchNumber, bytes calldata witness)
        external
        view
        returns (StateRecord memory record);

    function getPosition(bytes32 marketId, bytes32 positionId, uint64 batchNumber, bytes calldata witness)
        external
        view
        returns (StateRecord memory record);

    function getMargin(bytes32 account, bytes32 assetId, uint64 batchNumber, bytes calldata witness)
        external
        view
        returns (Margin memory margin);
}
