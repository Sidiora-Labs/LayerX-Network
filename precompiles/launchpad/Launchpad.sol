// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant LAUNCHPAD_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001017;

ILaunchpad constant LAUNCHPAD_CONTRACT = ILaunchpad(LAUNCHPAD_PRECOMPILE_ADDRESS);

/// Native KindleLaunch launchpad. Every market is a fixed-supply, 6-decimal
/// tokenfactory denom whose whole supply the launchpad module account holds
/// against a virtual-reserve constant-product curve priced in the quote
/// denom (USDL). token is the denom's ERC20 pointer.
///
/// Swaps follow SidioraPool.swap: the dynamic fee is charged in the input
/// token; a buy's fee is split protocolFeeBps to the treasury and the rest to
/// the market's fee-rights holder, a sell's fee stays in the token reserve,
/// and a sell never pays out more than the real quote balance (the virtual
/// floor). Accumulated buy fees follow the fee strategy the fee-rights holder
/// chose: 0 claim, 1 burn to 0x…dEaD, 2 airdrop to token holders by epoch,
/// 3 return to the curve's real quote balance.
///
/// Quote and tokens move from and to the bank accounts of msg.sender and
/// recipient; amounts are bank base units. Params change only through
/// governance.
///
/// Gas = 3000 + 16 * len(calldata after the selector) + 5000 * writes
///     + 9000000 for createMarket's pointer deployment.
/// writes: createMarket 12, buy 8, sell 6, claimFees, executeBurn 3,
/// executeAirdrop, claimAirdrop 4, setFeeStrategy, executeLpRewards, pause,
/// unpause 1, views 0.
interface ILaunchpad {
    struct Market {
        address token;
        string denom;
        uint64 index;
        string name;
        string symbol;
        address creator;
        address guardian;
        address feeRightsHolder;
        uint8 feeStrategy;
        bool paused;
        uint256 totalSupply;
        uint256 virtualQuoteReserve;
        uint256 realQuoteBalance;
        uint256 tokenReserve;
        uint256 createdAt;
        uint256 cumulativeVolume;
        uint256 accumulatedQuoteFees;
        uint256 accumulatedTokenFees;
        uint256 accumulatedFees;
        uint256 airdropEpoch;
        uint256 airdropBalance;
        uint256 price;
    }

    struct Config {
        string quoteDenom;
        uint256 virtualQuoteDefault;
        uint256 virtualTokenDefault;
        uint256 minFeeBps;
        uint256 maxFeeBps;
        uint256 baseFeeBps;
        uint256 protocolFeeBps;
        uint256 feeDecayRate;
        uint256 volatilityWeight;
        uint256 concentrationWeight;
        uint256 creationFee;
        uint256 protocolFeesPending;
    }

    event MarketCreated(address indexed token, address indexed creator, string denom, string name, string symbol, uint8 feeStrategy);
    event Swap(address indexed token, address indexed trader, address indexed recipient, bool isBuy, uint256 amountIn, uint256 amountOut, uint256 feeAmount, uint256 price);
    event FeeRecorded(address indexed token, uint256 feeAmount, uint256 protocolCut, uint256 poolCut);
    event FeeStrategyChanged(address indexed token, uint8 oldStrategy, uint8 newStrategy);
    event FeesClaimed(address indexed token, address indexed recipient, uint256 amount);
    event FeesBurned(address indexed token, uint256 amount);
    event AirdropExecuted(address indexed token, uint256 amount, uint256 epoch);
    event AirdropClaimed(address indexed token, address indexed holder, uint256 amount, uint256 epoch);
    event LpRewardsExecuted(address indexed token, uint256 amount);
    event PauseToggled(address indexed token, bool paused);

    function createMarket(string calldata name, string calldata symbol, uint8 feeStrategy) external returns (address token, string memory denom);
    function buy(address token, uint256 quoteIn, uint256 minOut, address recipient, uint256 deadline) external returns (uint256 amountOut);
    function sell(address token, uint256 amountIn, uint256 minOut, address recipient, uint256 deadline) external returns (uint256 amountOut);
    function setFeeStrategy(address token, uint8 feeStrategy) external returns (uint8 oldStrategy);
    function claimFees(address token, address recipient) external returns (uint256 amount);
    function executeBurn(address token) external returns (uint256 amount);
    function executeAirdrop(address token) external returns (uint256 amount);
    function claimAirdrop(address token) external returns (uint256 amount);
    function executeLpRewards(address token) external returns (uint256 amount);
    function pause(address token) external returns (bool);
    function unpause(address token) external returns (bool);

    function quoteBuy(address token, uint256 quoteIn) external view returns (uint256 amountOut, uint256 feeBps, uint256 feeAmount);
    function quoteSell(address token, uint256 amountIn) external view returns (uint256 amountOut, uint256 feeBps, uint256 feeAmount);
    function getReserves(address token) external view returns (uint256 virtualQuote, uint256 realQuote, uint256 tokenReserve);
    function getPrice(address token) external view returns (uint256);
    function getPriceSnapshots(address token) external view returns (uint256[8] memory snapshots, uint256 index, uint256 count);
    function getFeeBps(address token) external view returns (uint256);
    function getMarket(address token) external view returns (Market memory);
    function getMarkets(uint256 offset, uint256 limit) external view returns (Market[] memory);
    function getMarketsByCreator(address creator) external view returns (Market[] memory);
    function getMarketCount() external view returns (uint256);
    function getAccumulatedFees(address token) external view returns (uint256);
    function getConfig() external view returns (Config memory);
}
