// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import "@openzeppelin/contracts/utils/cryptography/MessageHashUtils.sol";
import "@openzeppelin/contracts/utils/math/Math.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IOracle} from "./precompiles/IOracle.sol";

/**
 * @title BatchCallAndSponsor
 * @notice EIP-7702 account batches with optional Sidiora sponsorship.
 * @dev Sponsored execution consumes both nonces before calls and repays the
 * sponsor after all calls. Any failed call or repayment reverts the whole batch.
 * Digests use abi.encode and the Ethereum signed-message prefix for 32 bytes.
 */
contract BatchCallAndSponsor {
    using ECDSA for bytes32;

    /// @notice A nonce used for replay protection.
    uint256 public nonce;

    /// @notice Represents a single call within a batch.
    struct Call {
        address to;
        uint256 value;
        bytes data;
    }

    struct Quote {
        address sponsor;
        address token;
        uint256 maxTokenAmount;
        uint256 tokenAmount;
        uint256 deadline;
        uint256 quoteNonce;
        uint256 gasCost;
    }

    address public constant SIDIORA = 0x21f7b20a555199fa73A238B1a91FD0f549068fEe;
    address public constant ORACLE = 0x0000000000000000000000000000000000001008;
    string public constant SID_ORACLE_DENOM = "usid";
    string public constant PAX_ORACLE_DENOM = "uhpx";
    uint256 public constant MAX_RATE_AGE = 300;
    uint256 public constant MAX_SPREAD_BPS = 500;
    bytes32 public constant QUOTE_TYPEHASH = keccak256(
        "Quote(uint256 chainId,address account,address sponsor,address token,uint256 maxTokenAmount,uint256 tokenAmount,uint256 deadline,uint256 quoteNonce,uint256 gasCost)"
    );
    bytes32 public constant BATCH_TYPEHASH =
        keccak256("SponsoredBatch(uint256 nonce,bytes32 callsHash,bytes32 quoteDigest)");

    mapping(address => mapping(uint256 => bool)) public usedQuoteNonces;

    error InvalidAccountSignature();
    error InvalidRelayerSignature();
    error InvalidQuote();
    error QuoteExpired();
    error QuoteAboveMaximum();
    error QuoteAlreadyUsed();
    error OracleUnavailable();
    error InvalidOracleRate();
    error StaleOracleRate();
    error QuoteOutsideSpread();
    error TokenTransferFailed();

    event Sponsored(address indexed sponsor, address indexed token, uint256 tokenAmount, uint256 quoteNonce);

    /// @notice gasCost is the declared fee in PAX wei; token amounts are SID base units.
    /// @dev Both oracle denoms carry prices in the same quote currency, as decimal strings.
    function quoteDigest(Quote calldata quote) public view returns (bytes32) {
        return MessageHashUtils.toEthSignedMessageHash(
            keccak256(abi.encode(QUOTE_TYPEHASH, block.chainid, address(this), quote))
        );
    }

    function sponsoredBatchDigest(Call[] calldata calls, Quote calldata quote) public view returns (bytes32) {
        return MessageHashUtils.toEthSignedMessageHash(
            keccak256(abi.encode(BATCH_TYPEHASH, nonce, keccak256(abi.encode(calls)), quoteDigest(quote)))
        );
    }

    function executeSponsored(
        Call[] calldata calls,
        Quote calldata quote,
        bytes calldata accountSignature,
        bytes calldata relayerSignature
    ) external payable {
        if (
            quote.sponsor == address(0) || quote.sponsor == address(this) || quote.token != SIDIORA
                || quote.gasCost == 0 || quote.tokenAmount == 0
        ) revert InvalidQuote();
        if (block.timestamp > quote.deadline) revert QuoteExpired();
        if (quote.tokenAmount > quote.maxTokenAmount) revert QuoteAboveMaximum();
        if (usedQuoteNonces[quote.sponsor][quote.quoteNonce]) revert QuoteAlreadyUsed();

        (address account, ECDSA.RecoverError accountError,) =
            ECDSA.tryRecover(sponsoredBatchDigest(calls, quote), accountSignature);
        if (accountError != ECDSA.RecoverError.NoError || account != address(this)) {
            revert InvalidAccountSignature();
        }
        (address sponsor, ECDSA.RecoverError sponsorError,) = ECDSA.tryRecover(quoteDigest(quote), relayerSignature);
        if (sponsorError != ECDSA.RecoverError.NoError || sponsor != quote.sponsor) {
            revert InvalidRelayerSignature();
        }

        _checkPrice(quote);
        usedQuoteNonces[quote.sponsor][quote.quoteNonce] = true;
        _executeBatch(calls);

        (bool success, bytes memory result) =
            quote.token.call(abi.encodeCall(IERC20.transfer, (quote.sponsor, quote.tokenAmount)));
        if (!success || result.length != 32 || abi.decode(result, (uint256)) != 1) revert TokenTransferFailed();
        emit Sponsored(quote.sponsor, quote.token, quote.tokenAmount, quote.quoteNonce);
    }

    function _checkPrice(Quote calldata quote) internal view {
        IOracle.DenomOracleExchangeRatePair[] memory rates;
        try IOracle(ORACLE).getExchangeRates() returns (IOracle.DenomOracleExchangeRatePair[] memory result) {
            rates = result;
        } catch {
            revert OracleUnavailable();
        }
        uint256 sidRate;
        uint256 paxRate;
        for (uint256 i; i < rates.length; i++) {
            bytes32 denom = keccak256(bytes(rates[i].denom));
            if (denom == keccak256(bytes(SID_ORACLE_DENOM))) {
                if (sidRate != 0) revert InvalidOracleRate();
                sidRate = _readRate(rates[i].oracleExchangeRateVal);
            } else if (denom == keccak256(bytes(PAX_ORACLE_DENOM))) {
                if (paxRate != 0) revert InvalidOracleRate();
                paxRate = _readRate(rates[i].oracleExchangeRateVal);
            }
        }
        if (sidRate == 0 || paxRate == 0) revert OracleUnavailable();
        uint256 sidWei = Math.mulDiv(quote.gasCost, paxRate, sidRate, Math.Rounding.Ceil);
        uint256 expected = Math.ceilDiv(sidWei, 1e12);
        uint256 lower = Math.mulDiv(expected, 10_000 - MAX_SPREAD_BPS, 10_000, Math.Rounding.Ceil);
        uint256 upper = Math.mulDiv(expected, 10_000 + MAX_SPREAD_BPS, 10_000);
        if (quote.tokenAmount < lower || quote.tokenAmount > upper) revert QuoteOutsideSpread();
    }

    function _readRate(IOracle.OracleExchangeRate memory rate) internal view returns (uint256) {
        if (
            rate.lastUpdateTimestamp <= 0 || uint256(uint64(rate.lastUpdateTimestamp)) > block.timestamp
                || block.timestamp - uint256(uint64(rate.lastUpdateTimestamp)) > MAX_RATE_AGE
        ) {
            revert StaleOracleRate();
        }
        bytes memory raw = bytes(rate.exchangeRate);
        uint256 value;
        uint256 decimals;
        bool dot;
        if (raw.length == 0 || raw[0] == bytes1(".") || raw[raw.length - 1] == bytes1(".")) {
            revert InvalidOracleRate();
        }
        for (uint256 i; i < raw.length; i++) {
            if (raw[i] == bytes1(".") && !dot) {
                dot = true;
                continue;
            }
            uint8 digit = uint8(raw[i]);
            if (digit < 48 || digit > 57 || (dot && ++decimals > 18) || value > (type(uint256).max - (digit - 48)) / 10)
            {
                revert InvalidOracleRate();
            }
            value = value * 10 + digit - 48;
        }
        uint256 scale = 10 ** (18 - decimals);
        if (value == 0 || value > type(uint256).max / scale) revert InvalidOracleRate();
        return value * scale;
    }

    /// @notice Emitted for every individual call executed.
    event CallExecuted(address indexed sender, address indexed to, uint256 value, bytes data);
    /// @notice Emitted when a full batch is executed.
    event BatchExecuted(uint256 indexed nonce, Call[] calls);

    /**
     * @notice Executes a batch of calls directly.
     * @dev This function is intended for use when the smart account itself (i.e. address(this))
     * calls the contract. It checks that msg.sender is the contract itself.
     * @param calls An array of Call structs containing destination, ETH value, and calldata.
     */
    function execute(Call[] calldata calls) external payable {
        require(msg.sender == address(this), "Invalid authority");
        _executeBatch(calls);
    }

    /**
     * @dev Internal function that handles batch execution and nonce incrementation.
     * @param calls An array of Call structs.
     */
    function _executeBatch(Call[] calldata calls) internal {
        uint256 currentNonce = nonce;
        nonce++; // Increment nonce to protect against replay attacks

        for (uint256 i = 0; i < calls.length; i++) {
            _executeCall(calls[i]);
        }

        emit BatchExecuted(currentNonce, calls);
    }

    /**
     * @dev Internal function to execute a single call.
     * @param callItem The Call struct containing destination, value, and calldata.
     */
    function _executeCall(Call calldata callItem) internal {
        (bool success,) = callItem.to.call{value: callItem.value}(callItem.data);
        require(success, "Call reverted");
        emit CallExecuted(msg.sender, callItem.to, callItem.value, callItem.data);
    }

    // Allow the contract to receive ETH (e.g. from DEX swaps or other transfers).
    fallback() external payable {}
    receive() external payable {}
}
