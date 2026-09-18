// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {PaxeerBetaDeploymentValidator} from "../contracts/deployment/PaxeerBetaDeploymentValidator.sol";
import {Features} from "../contracts/config/Features.sol";
import {Constants} from "../contracts/libraries/Constants.sol";
import {GuarantorBond} from "../contracts/GuarantorBond.sol";
import {LayerXTimelock} from "../contracts/governance/LayerXTimelock.sol";
import {LayerXVault} from "../contracts/custody/LayerXVault.sol";
import {WithdrawalNullifierRegistry} from "../contracts/storage/WithdrawalNullifierRegistry.sol";
import {PaxeerBetaDeploy} from "../scripts/PaxeerBetaDeploy.s.sol";
import {BetaUsdl} from "./PaxeerBetaDeploymentValidator.t.sol";

interface BetaDeployVm {
    function addr(uint256 privateKey) external pure returns (address);
    function chainId(uint256 newChainId) external;
    function etch(address target, bytes calldata code) external;
    function revertToState(uint256 snapshotId) external returns (bool);
    function setEnv(string calldata name, string calldata value) external;
    function snapshotState() external returns (uint256);
    function toString(bytes32 value) external pure returns (string memory);
}

contract PaxeerBetaDeploySettlementWiringTest {
    BetaDeployVm private constant vm = BetaDeployVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    uint256 private constant DEPLOYER_KEY = uint256(keccak256("LXP/Paxeer/beta-deployment/test-operator"));
    address private constant FIRST_CONTROLLER = address(0xC01);
    address private constant SECOND_CONTROLLER = address(0xC02);
    uint256 private constant BOND_AMOUNT = 1_000_000;

    PaxeerBetaDeploy private runner;
    BetaUsdl private token;
    address private operator;

    function setUp() public {
        vm.chainId(125);
        vm.setEnv("EVM_WALLET_PRIVATE_KEY", vm.toString(bytes32(DEPLOYER_KEY)));
        operator = vm.addr(DEPLOYER_KEY);
        BetaUsdl implementation = new BetaUsdl();
        vm.etch(Constants.USDL_TOKEN, address(implementation).code);
        token = BetaUsdl(Constants.USDL_TOKEN);
        token.mint(FIRST_CONTROLLER, BOND_AMOUNT);
        token.mint(SECOND_CONTROLLER, BOND_AMOUNT);
        runner = new PaxeerBetaDeploy();
    }

    function testGenesisActivationAuthorizesSettlementConsumers() public {
        PaxeerBetaDeploy.Addresses memory addresses = _runGenesisActivation();

        WithdrawalNullifierRegistry nullifiers = WithdrawalNullifierRegistry(addresses.nullifierRegistry);
        require(addresses.withdrawalClaims != address(0) && addresses.emergencyExit != address(0), "consumers");
        require(nullifiers.consumer(addresses.withdrawalClaims), "withdrawal claims not a nullifier consumer");
        require(nullifiers.consumer(addresses.emergencyExit), "emergency exit not a nullifier consumer");
    }

    function testGenesisActivationAuthorizesVaultSettlementModulesAndSlashingAuthority() public {
        PaxeerBetaDeploy.Addresses memory addresses = _runGenesisActivation();

        LayerXVault vault = LayerXVault(payable(addresses.vault));
        require(vault.settlementModule(addresses.withdrawalClaims), "withdrawal claims cannot release custody");
        require(vault.settlementModule(addresses.emergencyExit), "emergency exit cannot release custody");
        require(
            GuarantorBond(payable(addresses.guarantorBond)).slashingAuthority() == addresses.challengeManager,
            "slashing authority not wired to the challenge manager"
        );
    }

    function _runGenesisActivation() private returns (PaxeerBetaDeploy.Addresses memory addresses) {
        PaxeerBetaDeploymentValidator.Input memory input = _input();
        PaxeerBetaDeploymentValidator.GuarantorInput[] memory guarantors = _guarantors();
        (bytes memory descriptor, bytes memory registration) = _artifacts();

        uint256 snapshot = vm.snapshotState();
        address predictedBond = runner.predictImmediateBetaGuarantorBondForProtocol(input, descriptor, registration, 3);
        require(vm.revertToState(snapshot), "snapshot revert");

        token.approveFor(FIRST_CONTROLLER, predictedBond, BOND_AMOUNT);
        token.approveFor(SECOND_CONTROLLER, predictedBond, BOND_AMOUNT);

        addresses = runner.deployImmediateBetaForProtocol(input, guarantors, descriptor, registration, 3);
        require(addresses.guarantorBond == predictedBond, "bond prediction drifted");

        LayerXTimelock timelock = LayerXTimelock(payable(addresses.timelock));
        uint256 genesisStartNonce = timelock.operationNonce();
        runner.executePermissionsAndScheduleGenesis(addresses, input, guarantors, 0);
        runner.executeGenesisActivation(addresses, input, guarantors, genesisStartNonce);
    }

    function _artifacts() private pure returns (bytes memory descriptor, bytes memory registration) {
        descriptor = abi.encodePacked(
            bytes4("LXGD"),
            bytes1(0x01),
            bytes4(uint32(42)),
            bytes32(uint256(1)),
            bytes32(uint256(2)),
            bytes32(uint256(3))
        );
        registration = abi.encodePacked(
            bytes4("LXRR"), bytes1(0x01), bytes4(uint32(42)), bytes32(uint256(2)), bytes32(uint256(3))
        );
    }

    function _guarantors() private pure returns (PaxeerBetaDeploymentValidator.GuarantorInput[] memory guarantors) {
        guarantors = new PaxeerBetaDeploymentValidator.GuarantorInput[](2);
        guarantors[0] = PaxeerBetaDeploymentValidator.GuarantorInput({
            guarantorId: bytes32(uint256(1)),
            signer: address(0x101),
            bondController: FIRST_CONTROLLER,
            joinedEpoch: 1,
            governanceSequence: 1,
            bondAmount: BOND_AMOUNT
        });
        guarantors[1] = PaxeerBetaDeploymentValidator.GuarantorInput({
            guarantorId: bytes32(uint256(2)),
            signer: address(0x102),
            bondController: SECOND_CONTROLLER,
            joinedEpoch: 1,
            governanceSequence: 2,
            bondAmount: BOND_AMOUNT
        });
    }

    function _input() private view returns (PaxeerBetaDeploymentValidator.Input memory) {
        return PaxeerBetaDeploymentValidator.Input({
            release: "1.0.0",
            bootstrapOperator: operator,
            finalProposer: address(0xA11CE),
            finalExecutor: address(0xE0EC),
            emergencyCouncil: address(0xEC01),
            timelockDelay: 0,
            timelockGracePeriod: 7 days,
            timelockMaximumCallValue: 10 ether,
            usdlMinimumDeposit: 1_000_000,
            usdlCustodyCap: 1_000_000_000_000,
            challengeWindow: 7 days,
            checkpointLivenessBound: 1 days,
            minimumBondBps: 100,
            unbondingDelay: 7 days,
            checkpointThresholdNumerator: 2,
            checkpointThresholdDenominator: 3,
            checkpointMaximumAge: 1 hours,
            checkpointFutureDrift: 5 minutes,
            challengeBond: 1 ether,
            emergencyDelay: 1 days,
            migrationDelay: 1 days,
            migrationExpiry: 7 days,
            migrationGasLimit: 1_000_000,
            migrationMaximumCallValue: 1 ether,
            enabledFeatures: Features.ERC20_CUSTODY | Features.CHECKPOINT_CHALLENGES | Features.WITHDRAWAL_CLAIMS
                | Features.EMERGENCY_EXIT | Features.RESERVE_RECONCILIATION
        });
    }
}
