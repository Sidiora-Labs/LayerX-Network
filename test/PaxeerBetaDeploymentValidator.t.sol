// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {PaxeerBetaDeploymentValidator} from "../contracts/deployment/PaxeerBetaDeploymentValidator.sol";
import {StaticConfig} from "../contracts/config/StaticConfig.sol";
import {Features} from "../contracts/config/Features.sol";
import {PaxeerBetaDeploy} from "../script/PaxeerBetaDeploy.s.sol";

interface BetaDeploymentVm {
    function chainId(uint256 newChainId) external;
    function etch(address target, bytes calldata code) external;
    function expectPartialRevert(bytes4 selector) external;
}

contract BetaUsdl {
    uint8 public constant decimals = 6;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address recipient, uint256 amount) external {
        balanceOf[recipient] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function approveFor(address owner, address spender, uint256 amount) external {
        allowance[owner][spender] = amount;
    }
}

contract WrongDecimalsUsdl {
    uint8 public constant decimals = 18;
}

contract PaxeerBetaDeploymentValidatorHarness {
    function decode(bytes calldata descriptor, bytes calldata registrationRequest)
        external
        pure
        returns (PaxeerBetaDeploymentValidator.GenesisArtifacts memory)
    {
        return PaxeerBetaDeploymentValidator.decodeAndCrossCheckGenesis(descriptor, registrationRequest);
    }

    function validate(
        PaxeerBetaDeploymentValidator.Input calldata input,
        PaxeerBetaDeploymentValidator.GenesisArtifacts calldata genesis
    ) external view returns (uint192 release, StaticConfig.Config memory config) {
        return PaxeerBetaDeploymentValidator.validateInput(input, genesis);
    }

    function validateGuarantors(
        PaxeerBetaDeploymentValidator.GuarantorInput[] calldata guarantors,
        address guarantorBond
    ) external view {
        PaxeerBetaDeploymentValidator.validateGuarantors(guarantors, guarantorBond);
    }
}

contract PaxeerBetaDeploymentValidatorTest {
    BetaDeploymentVm private constant vm = BetaDeploymentVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    address private constant USDL = 0x85FcD13735F4309833A503EE804ea32395851479;
    address private constant BOND = address(0xB0D0);
    address private constant CONTROLLER = address(0xC01);

    PaxeerBetaDeploymentValidatorHarness private harness;
    BetaUsdl private token;

    function setUp() public {
        vm.chainId(125);
        harness = new PaxeerBetaDeploymentValidatorHarness();
        BetaUsdl implementation = new BetaUsdl();
        vm.etch(USDL, address(implementation).code);
        token = BetaUsdl(USDL);
    }

    function testCanonicalDescriptorAndRegistrationRequestCrossCheck() public view {
        (bytes memory descriptor, bytes memory registration) = _artifacts();
        PaxeerBetaDeploymentValidator.GenesisArtifacts memory result = harness.decode(descriptor, registration);
        require(result.networkId == 42, "network");
        require(result.manifestDigest == bytes32(uint256(1)), "manifest");
        require(result.canonicalStateRoot == bytes32(uint256(2)), "canonical");
        require(result.receiptRoot == bytes32(uint256(3)), "receipt");
    }

    function testRejectsDescriptorRegistrationMismatchAndMalformedArtifacts() public {
        (bytes memory descriptor, bytes memory registration) = _artifacts();
        registration[9] = bytes1(uint8(registration[9]) ^ 1);
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.InvalidGenesisArtifacts.selector);
        harness.decode(descriptor, registration);
        (, registration) = _artifacts();
        descriptor[0] = 0;
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.InvalidGenesisArtifacts.selector);
        harness.decode(descriptor, registration);
    }

    function testRejectsNonPaxeerChainAndWrongUsdlDecimals() public {
        PaxeerBetaDeploymentValidator.GenesisArtifacts memory genesis = _genesis();
        PaxeerBetaDeploymentValidator.Input memory input = _input();
        vm.chainId(126);
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.WrongPaxeerChain.selector);
        harness.validate(input, genesis);
        vm.chainId(125);
        WrongDecimalsUsdl wrong = new WrongDecimalsUsdl();
        vm.etch(USDL, address(wrong).code);
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.InvalidUsdl.selector);
        harness.validate(input, genesis);
    }

    function testBuildsExactProtocolV2StaticConfig() public view {
        (uint192 release, StaticConfig.Config memory config) = harness.validate(_input(), _genesis());
        require(release == (uint192(1) << 128), "release");
        require(config.chainId == 125 && config.protocolVersion == 2, "domain");
        require(config.governanceTimelock == address(0), "blueprint timelock must derive");
        require(config.usdlToken == USDL && config.usdlDecimals == 6, "USDL");
        require(config.genesisReceiptRoot == bytes32(uint256(3)), "receipt");
    }

    function testRejectsInvalidAuthorityEconomicAndReleaseInputs() public {
        PaxeerBetaDeploymentValidator.Input memory input = _input();
        input.finalExecutor = input.bootstrapOperator;
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.InvalidBetaDeploymentInput.selector);
        harness.validate(input, _genesis());
        input = _input();
        input.usdlCustodyCap = input.usdlMinimumDeposit - 1;
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.InvalidBetaDeploymentInput.selector);
        harness.validate(input, _genesis());
        input = _input();
        input.release = "01.0.0";
        vm.expectPartialRevert(bytes4(keccak256("InvalidSemanticVersion()")));
        harness.validate(input, _genesis());
    }

    function testGuarantorsMustBeSortedUniqueFundedAndApproved() public {
        PaxeerBetaDeploymentValidator.GuarantorInput[] memory guarantors = _guarantors();
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.UnfundedGuarantor.selector);
        harness.validateGuarantors(guarantors, BOND);
        token.mint(CONTROLLER, 2_000_000);
        token.approveFor(CONTROLLER, BOND, 2_000_000);
        token.approveFor(address(0xC02), BOND, 1_000_000);
        harness.validateGuarantors(guarantors, BOND);
        PaxeerBetaDeploymentValidator.GuarantorInput memory first = guarantors[0];
        guarantors[0] = guarantors[1];
        guarantors[1] = first;
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.InvalidGuarantor.selector);
        harness.validateGuarantors(guarantors, BOND);
    }

    function testRejectsEmptyGuarantorSet() public {
        PaxeerBetaDeploymentValidator.GuarantorInput[] memory guarantors =
            new PaxeerBetaDeploymentValidator.GuarantorInput[](0);
        vm.expectPartialRevert(PaxeerBetaDeploymentValidator.InvalidBetaDeploymentInput.selector);
        harness.validateGuarantors(guarantors, BOND);
    }

    function testRunnerRejectsMissingTopologyBeforeEveryStatefulPhase() public {
        PaxeerBetaDeploy runner = new PaxeerBetaDeploy();
        PaxeerBetaDeploy.Addresses memory addresses;
        PaxeerBetaDeploymentValidator.GuarantorInput[] memory guarantors =
            new PaxeerBetaDeploymentValidator.GuarantorInput[](0);
        vm.expectPartialRevert(PaxeerBetaDeploy.InvalidDeploymentState.selector);
        runner.executePermissionsAndScheduleGenesis(addresses, _input(), guarantors, 0);
        vm.expectPartialRevert(PaxeerBetaDeploy.InvalidDeploymentState.selector);
        runner.executeGenesisActivation(addresses, _input(), guarantors, 0);
        vm.expectPartialRevert(PaxeerBetaDeploy.InvalidDeploymentState.selector);
        runner.finalize(addresses, _input(), guarantors, 0);
        vm.expectPartialRevert(PaxeerBetaDeploy.InvalidDeploymentState.selector);
        runner.depositGenesisBond(address(0), bytes32(0), 0);
    }

    function testRunnerPhaseSelectorsAreStableAndDistinct() public pure {
        bytes4[5] memory selectors = [
            PaxeerBetaDeploy.deploy.selector,
            PaxeerBetaDeploy.executePermissionsAndScheduleGenesis.selector,
            PaxeerBetaDeploy.executeGenesisActivation.selector,
            PaxeerBetaDeploy.depositGenesisBond.selector,
            PaxeerBetaDeploy.finalize.selector
        ];
        for (uint256 i = 0; i < selectors.length; ++i) {
            require(selectors[i] != bytes4(0), "zero selector");
            for (uint256 j = 0; j < i; ++j) {
                require(selectors[i] != selectors[j], "selector collision");
            }
        }
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

    function _genesis() private pure returns (PaxeerBetaDeploymentValidator.GenesisArtifacts memory) {
        return PaxeerBetaDeploymentValidator.GenesisArtifacts({
            networkId: 42,
            manifestDigest: bytes32(uint256(1)),
            canonicalStateRoot: bytes32(uint256(2)),
            receiptRoot: bytes32(uint256(3))
        });
    }

    function _guarantors() private returns (PaxeerBetaDeploymentValidator.GuarantorInput[] memory guarantors) {
        guarantors = new PaxeerBetaDeploymentValidator.GuarantorInput[](2);
        guarantors[0] = PaxeerBetaDeploymentValidator.GuarantorInput({
            guarantorId: bytes32(uint256(1)),
            signer: address(0x101),
            bondController: CONTROLLER,
            joinedEpoch: 1,
            governanceSequence: 1,
            bondAmount: 1_000_000
        });
        guarantors[1] = PaxeerBetaDeploymentValidator.GuarantorInput({
            guarantorId: bytes32(uint256(2)),
            signer: address(0x102),
            bondController: address(0xC02),
            joinedEpoch: 1,
            governanceSequence: 2,
            bondAmount: 1_000_000
        });
        token.mint(address(0xC02), 1_000_000);
    }

    function _input() private view returns (PaxeerBetaDeploymentValidator.Input memory) {
        return PaxeerBetaDeploymentValidator.Input({
            release: "1.0.0",
            bootstrapOperator: address(this),
            finalProposer: address(0xA11CE),
            finalExecutor: address(0xE0EC),
            emergencyCouncil: address(0xEC01),
            timelockDelay: 1 days,
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
