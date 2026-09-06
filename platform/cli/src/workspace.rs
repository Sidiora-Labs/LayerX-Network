use std::collections::BTreeSet;
use std::env;
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use crate::config::{Configuration, Environment};
use crate::output::CommandOutput;

#[derive(Args, Debug)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    command: Option<WorkspaceCommand>,
}

#[derive(Subcommand, Debug)]
enum WorkspaceCommand {
    /// List every module controlled by the workspace CLI.
    Modules,
    /// Inspect system tools and module readiness without changing anything.
    Doctor(SelectionArgs),
    /// Resolve locked project dependencies for selected modules.
    Install(RunArgs),
    /// Build selected modules.
    Build(RunArgs),
    /// Test selected modules.
    Test(RunArgs),
    /// Install dependencies, build, and test selected modules in order.
    All(RunArgs),
}

#[derive(Args, Clone, Debug, Default)]
struct SelectionArgs {
    /// Module ids, repeatable or comma-separated.
    #[arg(short = 'm', long = "module", value_delimiter = ',')]
    modules: Vec<String>,
    /// Select every declared module.
    #[arg(long)]
    all: bool,
    /// `LayerX` environment exposed to builds and tests.
    #[arg(long)]
    environment: Option<String>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Args, Clone, Debug, Default)]
struct RunArgs {
    #[command(flatten)]
    selection: SelectionArgs,
    /// Print the complete execution plan without running commands.
    #[arg(long)]
    dry_run: bool,
    /// Stop immediately instead of completing independent module steps.
    #[arg(long)]
    fail_fast: bool,
    /// Run without the final confirmation prompt.
    #[arg(short = 'y', long)]
    yes: bool,
    /// Export CI=1 to every child command.
    #[arg(long)]
    ci: bool,
    /// Permit test commands to receive the production environment.
    #[arg(long)]
    allow_production: bool,
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Doctor,
    Install,
    Build,
    Test,
    All,
}

impl Action {
    const fn label(self) -> &'static str {
        match self {
            Self::Doctor => "doctor",
            Self::Install => "install",
            Self::Build => "build",
            Self::Test => "test",
            Self::All => "install + build + test",
        }
    }

    const fn phases(self) -> &'static [Phase] {
        match self {
            Self::Doctor => &[],
            Self::Install => &[Phase::Install],
            Self::Build => &[Phase::Build],
            Self::Test => &[Phase::Test],
            Self::All => &[Phase::Install, Phase::Build, Phase::Test],
        }
    }

    const fn can_touch_network(self) -> bool {
        matches!(self, Self::Install | Self::All)
    }

    const fn can_test(self) -> bool {
        matches!(self, Self::Test | Self::All)
    }
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    Install,
    Build,
    Test,
}

impl Phase {
    const fn label(self) -> &'static str {
        match self {
            Self::Install => "INSTALL",
            Self::Build => "BUILD",
            Self::Test => "TEST",
        }
    }
}

#[derive(Clone, Copy)]
struct Step {
    label: &'static str,
    program: Option<&'static str>,
    args: &'static [&'static str],
    cwd: &'static str,
    requires: &'static [&'static str],
}

impl Step {
    const fn command(
        label: &'static str,
        program: &'static str,
        args: &'static [&'static str],
        cwd: &'static str,
        requires: &'static [&'static str],
    ) -> Self {
        Self {
            label,
            program: Some(program),
            args,
            cwd,
            requires,
        }
    }

    const fn no_op(label: &'static str) -> Self {
        Self {
            label,
            program: None,
            args: &[],
            cwd: ".",
            requires: &[],
        }
    }
}

struct Module {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    tools: &'static [&'static str],
    install: &'static [Step],
    build: &'static [Step],
    test: &'static [Step],
}

const CORE_INSTALL: &[Step] = &[Step::no_op("No project-managed dependencies")];
const CORE_BUILD: &[Step] = &[Step::command(
    "Compile the C17 protocol core",
    "make",
    &["build"],
    ".",
    &["make", "cc", "ar"],
)];
const CORE_TEST: &[Step] = &[
    Step::command(
        "Run the protocol foundation suites",
        "make",
        &["test"],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test the kernel and module boundary",
        "make",
        &[
            "test-kernel",
            "test-module-ctx",
            "test-dispatch",
            "test-receipts",
            "test-state-root",
        ],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test the ledger and asset module",
        "make",
        &[
            "test-ledger-accounts",
            "test-ledger-transfer",
            "test-ledger-set",
            "test-ledger-send",
            "test-ledger-receive",
            "test-ledger-receipt",
            "test-asset-registry",
            "test-asset-balance",
            "test-asset-transfer",
            "test-asset-deposit",
            "test-asset-withdraw",
            "test-asset-reserve",
        ],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test escrow, budget, and stream modules",
        "make",
        &[
            "test-escrow-open",
            "test-escrow-capture",
            "test-escrow-timeout",
            "test-escrow-dispute",
            "test-escrow-invariants",
            "test-budget-create",
            "test-budget-period",
            "test-budget-spend",
            "test-budget-delegate",
            "test-budget-revoke",
            "test-stream-open",
            "test-stream-accrual",
            "test-stream-meter",
            "test-stream-settle",
            "test-stream-lifecycle",
        ],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test service and oracle modules",
        "make",
        &["test-wave-8"],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test the perps module",
        "make",
        &["test-wave-9"],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test batches and sequencing",
        "make",
        &["test-wave-10"],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test replicas, replay, and recovery",
        "make",
        &["test-wave-11"],
        ".",
        &["make", "cc", "ar", "docker"],
    ),
    Step::command(
        "Test guarantors, availability, governance, and fees",
        "make",
        &["test-wave-12"],
        ".",
        &["make", "cc", "ar"],
    ),
    Step::command(
        "Test custody, gateways, tools, and operator recovery",
        "make",
        &[
            "test-paxeer",
            "test-paxeer-bond",
            "test-bridge-deposit",
            "test-bridge-withdraw",
            "test-emergency-exit",
            "test-reserve",
            "test-gateway",
            "test-gateway-send",
            "test-gateway-receive",
            "test-receipt-offline",
            "test-layerxd",
            "test-tools",
            "test-genesis",
            "test-genesis-import",
            "test-genesis-reconcile",
            "test-legacy-readonly",
            "test-shadow",
        ],
        ".",
        &["make", "cc", "ar", "forge"],
    ),
];

const CONTRACTS_INSTALL: &[Step] = &[Step::no_op("Foundry resolves compiler inputs at build")];
const CONTRACTS_BUILD: &[Step] = &[Step::command(
    "Compile Paxeer contracts",
    "forge",
    &["build"],
    ".",
    &["forge"],
)];
const CONTRACTS_TEST: &[Step] = &[Step::command(
    "Run Paxeer contract tests",
    "forge",
    &["test"],
    ".",
    &["forge"],
)];

const AGENT_INSTALL: &[Step] = &[Step::command(
    "Fetch the locked agent workspace crates",
    "cargo",
    &["fetch", "--manifest-path", "agent/Cargo.toml", "--locked"],
    ".",
    &["cargo"],
)];
const AGENT_BUILD: &[Step] = &[Step::command(
    "Build every agent workspace crate",
    "make",
    &["agent-build"],
    ".",
    &["make", "cargo"],
)];
const AGENT_TEST: &[Step] = &[Step::command(
    "Test every agent workspace crate",
    "make",
    &["agent-test"],
    ".",
    &["make", "cargo"],
)];

const HUMAN_INSTALL: &[Step] = &[
    Step::command(
        "Fetch the locked human workspace crates",
        "cargo",
        &["fetch", "--manifest-path", "human/Cargo.toml", "--locked"],
        ".",
        &["cargo"],
    ),
    Step::command(
        "Install the locked human web packages",
        "npm",
        &[
            "--prefix",
            "human/apps/web",
            "ci",
            "--no-audit",
            "--no-fund",
        ],
        ".",
        &["npm"],
    ),
];
const HUMAN_BUILD: &[Step] = &[Step::command(
    "Build the human service and web application",
    "make",
    &["human-build"],
    ".",
    &["make", "cargo", "npm"],
)];
const HUMAN_TEST: &[Step] = &[Step::command(
    "Test the human service and web application",
    "make",
    &["human-test"],
    ".",
    &["make", "cargo", "npm"],
)];

const PLATFORM_INSTALL: &[Step] = &[
    Step::command(
        "Fetch the locked platform workspace crates",
        "cargo",
        &[
            "fetch",
            "--manifest-path",
            "platform/Cargo.toml",
            "--locked",
        ],
        ".",
        &["cargo"],
    ),
    Step::command(
        "Install the locked JavaScript workspaces",
        "npm",
        &[
            "ci",
            "--workspaces",
            "--include-workspace-root",
            "--no-audit",
            "--no-fund",
        ],
        ".",
        &["npm"],
    ),
    Step::command(
        "Download Go SDK modules",
        "go",
        &["mod", "download"],
        "platform/sdk/go",
        &["go"],
    ),
    Step::command(
        "Resolve JVM SDK modules",
        "mvn",
        &["-q", "dependency:go-offline"],
        "platform/sdk/jvm",
        &["mvn", "java"],
    ),
    Step::command(
        "Restore the C sharp SDK",
        "dotnet",
        &["restore", "LayerX.Sdk.csproj", "--nologo"],
        "platform/sdk/dotnet",
        &["dotnet"],
    ),
    Step::command(
        "Resolve Swift SDK packages",
        "swift",
        &["package", "resolve"],
        "platform/sdk/swift",
        &["swift"],
    ),
];
const PLATFORM_BUILD: &[Step] = &[
    Step::command(
        "Build the platform Rust workspace",
        "make",
        &["platform-build"],
        ".",
        &["make", "cargo", "python3"],
    ),
    Step::command(
        "Build every JavaScript middleware and integration",
        "npm",
        &["run", "build", "--workspaces", "--if-present"],
        ".",
        &["npm"],
    ),
    Step::command(
        "Build the Go SDK",
        "go",
        &["build", "./..."],
        "platform/sdk/go",
        &["go"],
    ),
    Step::command(
        "Build the JVM SDK",
        "mvn",
        &["-q", "-DskipTests", "package"],
        "platform/sdk/jvm",
        &["mvn", "java"],
    ),
    Step::command(
        "Build the C sharp SDK",
        "dotnet",
        &[
            "build",
            "LayerX.Sdk.csproj",
            "--configuration",
            "Release",
            "--nologo",
        ],
        "platform/sdk/dotnet",
        &["dotnet"],
    ),
    Step::command(
        "Build the Swift SDK",
        "swift",
        &["build"],
        "platform/sdk/swift",
        &["swift"],
    ),
    Step::command(
        "Compile the Python SDK and integrations",
        "python3",
        &[
            "-m",
            "compileall",
            "-q",
            "agent/sdk/python",
            "platform/integrations/fastapi",
        ],
        ".",
        &["python3"],
    ),
];
const PLATFORM_TEST: &[Step] = &[
    Step::command(
        "Test the platform Rust workspace",
        "make",
        &["platform-test"],
        ".",
        &["make", "cargo"],
    ),
    Step::command(
        "Test every generated SDK",
        "make",
        &["platform-test-sdks"],
        ".",
        &[
            "make", "cargo", "python3", "go", "mvn", "java", "dotnet", "swift",
        ],
    ),
    Step::command(
        "Test middleware and framework integrations",
        "make",
        &["platform-test-middleware"],
        ".",
        &["make", "npm", "python3", "mvn", "java", "swift"],
    ),
    Step::command(
        "Test the CLI, emulator, and hosted tooling",
        "make",
        &["platform-test-tooling"],
        ".",
        &["make", "cargo"],
    ),
    Step::command(
        "Build and execute documentation samples",
        "make",
        &["platform-test-docs"],
        ".",
        &[
            "make", "cargo", "npm", "python3", "go", "mvn", "java", "dotnet", "swift",
        ],
    ),
];

const PROGRAMS_INSTALL: &[Step] = &[
    Step::command(
        "Fetch the locked programs workspace crates",
        "cargo",
        &[
            "fetch",
            "--manifest-path",
            "programs/Cargo.toml",
            "--locked",
        ],
        ".",
        &["cargo"],
    ),
    Step::command(
        "Install the locked AssemblyScript SDK packages",
        "npm",
        &["ci", "--no-audit", "--no-fund"],
        "programs/sdk/assemblyscript",
        &["npm"],
    ),
    Step::command(
        "Install the locked AssemblyScript example packages",
        "npm",
        &["ci", "--no-audit", "--no-fund"],
        "programs/sdk/assemblyscript/examples/paid-counter",
        &["npm"],
    ),
];
const PROGRAMS_BUILD: &[Step] = &[
    Step::command(
        "Build the deterministic programs workspace",
        "make",
        &["programs-build"],
        ".",
        &["make", "cargo", "cc"],
    ),
    Step::command(
        "Build the AssemblyScript program SDK",
        "npm",
        &["run", "build"],
        "programs/sdk/assemblyscript/examples/paid-counter",
        &["npm"],
    ),
];
const PROGRAMS_TEST: &[Step] = &[Step::command(
    "Test the programs runtime and authoring SDKs",
    "make",
    &["programs-test"],
    ".",
    &["make", "cargo", "cc", "npm"],
)];

const INTEROP_INSTALL: &[Step] = &[Step::command(
    "Fetch the locked interop workspace crates",
    "cargo",
    &["fetch", "--manifest-path", "interop/Cargo.toml", "--locked"],
    ".",
    &["cargo"],
)];
const INTEROP_BUILD: &[Step] = &[Step::command(
    "Build every interop adapter",
    "make",
    &["interop-build"],
    ".",
    &["make", "cargo"],
)];
const INTEROP_TEST: &[Step] = &[Step::command(
    "Test every interop adapter",
    "make",
    &["interop-test"],
    ".",
    &["make", "cargo"],
)];

const SPECGEN_INSTALL: &[Step] = &[Step::command(
    "Download spec generator modules",
    "go",
    &["mod", "download"],
    "spec/specgen",
    &["go"],
)];
const SPECGEN_BUILD: &[Step] = &[Step::command(
    "Build the specification generator",
    "go",
    &["build", "./..."],
    "spec/specgen",
    &["go"],
)];
const SPECGEN_TEST: &[Step] = &[Step::command(
    "Test the specification generator",
    "go",
    &["test", "./..."],
    "spec/specgen",
    &["go"],
)];

static MODULES: &[Module] = &[
    Module {
        id: "core",
        name: "Protocol core",
        description: "C17 ledger, modules, sequencing, storage, and proofs",
        tools: &["make", "cc", "ar", "docker", "forge"],
        install: CORE_INSTALL,
        build: CORE_BUILD,
        test: CORE_TEST,
    },
    Module {
        id: "contracts",
        name: "Paxeer contracts",
        description: "Custody, bridge, governance, and challenge contracts",
        tools: &["forge"],
        install: CONTRACTS_INSTALL,
        build: CONTRACTS_BUILD,
        test: CONTRACTS_TEST,
    },
    Module {
        id: "agent",
        name: "Agent layer",
        description: "Daemon, SDK, wire, proof, policy, and MCP crates",
        tools: &["cargo"],
        install: AGENT_INSTALL,
        build: AGENT_BUILD,
        test: AGENT_TEST,
    },
    Module {
        id: "human",
        name: "Human plane",
        description: "Custody service, journeys, explorer, and web app",
        tools: &["cargo", "npm"],
        install: HUMAN_INSTALL,
        build: HUMAN_BUILD,
        test: HUMAN_TEST,
    },
    Module {
        id: "platform",
        name: "Developer platform",
        description: "CLI, hosted tooling, seven SDKs, middleware, and docs",
        tools: &[
            "make", "cargo", "npm", "python3", "go", "java", "mvn", "dotnet", "swift",
        ],
        install: PLATFORM_INSTALL,
        build: PLATFORM_BUILD,
        test: PLATFORM_TEST,
    },
    Module {
        id: "programs",
        name: "Programs",
        description: "Deterministic runtime, registry, SDKs, and porting kits",
        tools: &["cargo", "npm", "make", "cc"],
        install: PROGRAMS_INSTALL,
        build: PROGRAMS_BUILD,
        test: PROGRAMS_TEST,
    },
    Module {
        id: "interop",
        name: "Interop gateway",
        description: "x402, mandates, migration, fiat, mirrors, and ramps",
        tools: &["cargo", "make"],
        install: INTEROP_INSTALL,
        build: INTEROP_BUILD,
        test: INTEROP_TEST,
    },
    Module {
        id: "specgen",
        name: "Spec generator",
        description: "KVX specification rendering and workflow tooling",
        tools: &["go"],
        install: SPECGEN_INSTALL,
        build: SPECGEN_BUILD,
        test: SPECGEN_TEST,
    },
];

struct EnvironmentContext {
    name: String,
    endpoint: String,
    network_id: u32,
}

struct PlannedStep<'a> {
    module: &'a Module,
    phase: Phase,
    step: &'a Step,
}

struct StepResult {
    module: &'static str,
    phase: &'static str,
    label: &'static str,
    command: String,
    status: &'static str,
    duration_ms: u128,
    missing_tools: Vec<&'static str>,
}

pub fn run(arguments: WorkspaceArgs, machine: bool) -> Result<Option<CommandOutput>, String> {
    match arguments.command {
        None => interactive(machine),
        Some(WorkspaceCommand::Modules) => Ok(Some(list_modules(machine))),
        Some(WorkspaceCommand::Doctor(selection)) => doctor(&selection, machine).map(Some),
        Some(WorkspaceCommand::Install(arguments)) => execute(Action::Install, &arguments, machine),
        Some(WorkspaceCommand::Build(arguments)) => execute(Action::Build, &arguments, machine),
        Some(WorkspaceCommand::Test(arguments)) => execute(Action::Test, &arguments, machine),
        Some(WorkspaceCommand::All(arguments)) => execute(Action::All, &arguments, machine),
    }
}

fn interactive(machine: bool) -> Result<Option<CommandOutput>, String> {
    if machine || !io::stdin().is_terminal() {
        return Err("interactive workspace mode needs a terminal; choose modules, doctor, install, build, test, or all".into());
    }
    #[cfg(unix)]
    {
        let root = repo_root()?;
        let dashboard = root.join("layerx");
        if dashboard.is_file() {
            let status = Command::new(&dashboard)
                .current_dir(root)
                .status()
                .map_err(|error| format!("could not open {}: {error}", dashboard.display()))?;
            if status.success() {
                return Ok(None);
            }
            return Err(format!("workspace dashboard exited with status {status}"));
        }
    }
    let color = color_enabled();
    print_banner(color);
    let action = prompt_action()?;
    let modules = prompt_modules()?;
    let environment = prompt_environment()?;
    if matches!(action, Action::Doctor) {
        let selection = SelectionArgs {
            modules,
            all: false,
            environment,
        };
        return doctor(&selection, false).map(Some);
    }
    let selected_environment = environment_context(environment.as_deref())?;
    let allow_production = production_confirmation(action, &selected_environment.name)?;
    let arguments = RunArgs {
        selection: SelectionArgs {
            modules,
            all: false,
            environment,
        },
        dry_run: false,
        fail_fast: false,
        yes: false,
        ci: false,
        allow_production,
    };
    execute(action, &arguments, false)
}

fn list_modules(machine: bool) -> CommandOutput {
    if !machine {
        print_module_table(MODULES.iter());
    }
    let data = Value::Array(MODULES.iter().map(module_json).collect());
    CommandOutput::new(
        "workspace.modules",
        format!("{} LayerX modules", MODULES.len()),
        if machine { data } else { Value::Null },
    )
}

fn doctor(selection: &SelectionArgs, machine: bool) -> Result<CommandOutput, String> {
    let modules = select_modules(&selection.modules, selection.all, true)?;
    let environment = environment_context(selection.environment.as_deref())?;
    let mut tools = BTreeSet::new();
    for module in &modules {
        tools.extend(module.tools.iter().copied());
    }
    let checks = tools
        .into_iter()
        .map(|tool| (tool, command_exists(tool)))
        .collect::<Vec<_>>();
    if !machine {
        print_doctor(&modules, &checks, &environment);
    }
    let data = json!({
        "host": {"os": env::consts::OS, "arch": env::consts::ARCH},
        "environment": environment_json(&environment),
        "modules": modules.iter().map(|module| module.id).collect::<Vec<_>>(),
        "tools": checks.iter().map(|(tool, found)| json!({
            "name": tool,
            "available": found,
            "install_hint": (!found).then(|| install_hint(tool)),
        })).collect::<Vec<_>>(),
    });
    let missing = checks.iter().filter(|(_, found)| !found).count();
    Ok(CommandOutput::new(
        "workspace.doctor",
        if missing == 0 {
            "Every selected module is ready".to_string()
        } else {
            format!("{missing} required tools are missing")
        },
        if machine { data } else { Value::Null },
    ))
}

fn execute(
    action: Action,
    arguments: &RunArgs,
    machine: bool,
) -> Result<Option<CommandOutput>, String> {
    let modules = select_modules(&arguments.selection.modules, arguments.selection.all, false)?;
    let environment = environment_context(arguments.selection.environment.as_deref())?;
    enforce_environment_safety(action, arguments, &environment)?;
    let root = repo_root()?;
    let plan = create_plan(action, &modules);
    if !machine {
        print_plan(action, &plan, &environment, arguments.dry_run);
    }
    confirm_execution(action, arguments, machine)?;
    let results = run_plan(&plan, &root, &environment, arguments, machine)?;
    let failed = results
        .iter()
        .filter(|result| matches!(result.status, "failed" | "blocked"))
        .count();
    if !machine {
        print_summary(&results, failed);
    }
    if failed != 0 && !arguments.dry_run {
        return Err(format!("{failed} workspace steps did not complete"));
    }
    let data = json!({
        "action": action.label(),
        "dry_run": arguments.dry_run,
        "environment": environment_json(&environment),
        "steps": results.iter().map(result_json).collect::<Vec<_>>(),
    });
    Ok(Some(CommandOutput::new(
        "workspace.complete",
        if arguments.dry_run {
            format!("Planned {} workspace steps", results.len())
        } else {
            format!("Completed {} workspace steps", results.len())
        },
        if machine { data } else { Value::Null },
    )))
}

fn create_plan<'a>(action: Action, modules: &'a [&'a Module]) -> Vec<PlannedStep<'a>> {
    let mut plan = Vec::new();
    for phase in action.phases() {
        for module in modules {
            let steps = match phase {
                Phase::Install => module.install,
                Phase::Build => module.build,
                Phase::Test => module.test,
            };
            plan.extend(steps.iter().map(|step| PlannedStep {
                module,
                phase: *phase,
                step,
            }));
        }
    }
    plan
}

fn run_plan(
    plan: &[PlannedStep<'_>],
    root: &Path,
    environment: &EnvironmentContext,
    arguments: &RunArgs,
    machine: bool,
) -> Result<Vec<StepResult>, String> {
    let mut results = Vec::with_capacity(plan.len());
    for (index, planned) in plan.iter().enumerate() {
        let missing = planned
            .step
            .requires
            .iter()
            .copied()
            .filter(|tool| !command_exists(tool))
            .collect::<Vec<_>>();
        let result = run_step(
            planned,
            index,
            plan.len(),
            &missing,
            root,
            environment,
            arguments,
            machine,
        )?;
        let stop = matches!(result.status, "failed" | "blocked") && arguments.fail_fast;
        results.push(result);
        if stop {
            break;
        }
    }
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
fn run_step(
    planned: &PlannedStep<'_>,
    index: usize,
    total: usize,
    missing: &[&'static str],
    root: &Path,
    environment: &EnvironmentContext,
    arguments: &RunArgs,
    machine: bool,
) -> Result<StepResult, String> {
    let command = display_command(planned.step);
    if !machine {
        print_step(index + 1, total, planned, &command, missing);
    }
    if arguments.dry_run {
        return Ok(step_result(planned, command, "planned", 0, missing));
    }
    if !missing.is_empty() {
        return Ok(step_result(planned, command, "blocked", 0, missing));
    }
    let Some(program) = planned.step.program else {
        return Ok(step_result(planned, command, "not-needed", 0, missing));
    };
    let started = Instant::now();
    let mut child = Command::new(program);
    child
        .args(planned.step.args)
        .current_dir(root.join(planned.step.cwd))
        .env("LAYERX_ENVIRONMENT", &environment.name)
        .env("LAYERX_ENDPOINT", &environment.endpoint)
        .env("LAYERX_NETWORK_ID", environment.network_id.to_string());
    if arguments.ci {
        child.env("CI", "1");
    }
    if machine {
        child.stdout(Stdio::null()).stderr(Stdio::null());
    } else {
        child.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }
    let status = child
        .status()
        .map_err(|error| format!("could not run {command}: {error}"))?;
    let elapsed = started.elapsed().as_millis();
    Ok(step_result(
        planned,
        command,
        if status.success() { "passed" } else { "failed" },
        elapsed,
        missing,
    ))
}

fn step_result(
    planned: &PlannedStep<'_>,
    command: String,
    status: &'static str,
    duration_ms: u128,
    missing: &[&'static str],
) -> StepResult {
    StepResult {
        module: planned.module.id,
        phase: planned.phase.label(),
        label: planned.step.label,
        command,
        status,
        duration_ms,
        missing_tools: missing.to_vec(),
    }
}

fn select_modules(
    requested: &[String],
    all: bool,
    default_all: bool,
) -> Result<Vec<&'static Module>, String> {
    if all && !requested.is_empty() {
        return Err("use either --all or --module, not both".into());
    }
    if all || (requested.is_empty() && default_all) {
        return Ok(MODULES.iter().collect());
    }
    if requested.is_empty() {
        return Err("select at least one --module or pass --all".into());
    }
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for id in requested {
        let normalized = id.to_ascii_lowercase();
        let module = MODULES
            .iter()
            .find(|module| module.id == normalized)
            .ok_or_else(|| format!("unknown module {id}; run layerx workspace modules"))?;
        if seen.insert(module.id) {
            selected.push(module);
        }
    }
    Ok(selected)
}

fn environment_context(selected: Option<&str>) -> Result<EnvironmentContext, String> {
    let configuration = Configuration::load()?;
    let name = selected.unwrap_or(&configuration.current_environment);
    Configuration::validate_environment_name(name)?;
    let environment = configuration
        .environments
        .get(name)
        .ok_or_else(|| format!("environment {name} is not configured"))?;
    Ok(environment_from(name, environment))
}

fn environment_from(name: &str, environment: &Environment) -> EnvironmentContext {
    EnvironmentContext {
        name: name.to_owned(),
        endpoint: environment.endpoint.clone(),
        network_id: environment.network_id,
    }
}

fn enforce_environment_safety(
    action: Action,
    arguments: &RunArgs,
    environment: &EnvironmentContext,
) -> Result<(), String> {
    if environment.name == "production" && action.can_test() && !arguments.allow_production {
        return Err(
            "production tests require --allow-production; inspect the plan with --dry-run first"
                .into(),
        );
    }
    Ok(())
}

fn confirm_execution(action: Action, arguments: &RunArgs, machine: bool) -> Result<(), String> {
    if arguments.dry_run || arguments.yes {
        return Ok(());
    }
    if machine || !io::stdin().is_terminal() {
        return Err(
            "non-interactive execution requires --yes; use --dry-run to inspect first".into(),
        );
    }
    let scope = if action.can_touch_network() {
        "This can download locked project dependencies. Continue? [y/N] "
    } else {
        "Run this workspace plan? [y/N] "
    };
    if prompt(scope)?.eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        Err("workspace run cancelled".into())
    }
}

fn repo_root() -> Result<PathBuf, String> {
    if let Some(explicit) = env::var_os("LAYERX_REPO_ROOT") {
        let root = PathBuf::from(explicit);
        if is_repo_root(&root) {
            return Ok(root);
        }
        return Err(format!(
            "{} is not a LayerX repository root",
            root.display()
        ));
    }
    let current = env::current_dir().map_err(|error| format!("could not read cwd: {error}"))?;
    current
        .ancestors()
        .find(|candidate| is_repo_root(candidate))
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            "could not find the LayerX repository; run inside it or set LAYERX_REPO_ROOT".into()
        })
}

fn is_repo_root(path: &Path) -> bool {
    path.join("Makefile").is_file()
        && path.join("platform/Cargo.toml").is_file()
        && path.join("agent/Cargo.toml").is_file()
}

fn command_exists(tool: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|directory| executable_in(&directory, tool))
}

fn executable_in(directory: &Path, tool: &str) -> bool {
    let direct = directory.join(tool);
    if direct.is_file() {
        return true;
    }
    if cfg!(windows) {
        return env::var_os("PATHEXT").is_some_and(|extensions| {
            extensions
                .to_string_lossy()
                .split(';')
                .any(|extension| directory.join(format!("{tool}{extension}")).is_file())
        });
    }
    false
}

fn install_hint(tool: &str) -> &'static str {
    match tool {
        "cc" | "ar" | "make" => "Install the native C build toolchain for this operating system.",
        "cargo" => "Install the pinned Rust toolchain with rustup.",
        "npm" => "Install the project-supported Node.js release, which includes npm.",
        "python3" => "Install Python 3 for this operating system.",
        "go" => "Install the project-supported Go toolchain.",
        "java" | "mvn" => "Install JDK 21 and Maven.",
        "dotnet" => "Install the .NET 8 SDK.",
        "swift" => "Install the Swift toolchain supported by this operating system.",
        "forge" => "Install Foundry so forge is available on PATH.",
        "docker" => "Install a Docker-compatible container runtime.",
        _ => "Install this tool and make it available on PATH.",
    }
}

fn prompt_action() -> Result<Action, String> {
    println!("  1  Doctor       Inspect every prerequisite");
    println!("  2  Install      Resolve locked project dependencies");
    println!("  3  Build        Build selected modules");
    println!("  4  Test         Test selected modules");
    println!("  5  Everything   Install, build, then test\n");
    match prompt("Choose an action [1-5]: ")?.as_str() {
        "1" => Ok(Action::Doctor),
        "2" => Ok(Action::Install),
        "3" => Ok(Action::Build),
        "4" => Ok(Action::Test),
        "5" => Ok(Action::All),
        _ => Err("action must be a number from 1 to 5".into()),
    }
}

fn prompt_modules() -> Result<Vec<String>, String> {
    println!("\nModules");
    for (index, module) in MODULES.iter().enumerate() {
        println!("  {:>2}  {:<12} {}", index + 1, module.id, module.name);
    }
    let answer = prompt("Select numbers or ids separated by commas [all]: ")?;
    if answer.is_empty() || answer.eq_ignore_ascii_case("all") {
        return Ok(MODULES.iter().map(|module| module.id.to_owned()).collect());
    }
    answer
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<usize>().map_or_else(
                |_| Ok(value.to_owned()),
                |index| {
                    index
                        .checked_sub(1)
                        .and_then(|offset| MODULES.get(offset))
                        .map(|module| module.id.to_owned())
                        .ok_or_else(|| format!("module number {index} is out of range"))
                },
            )
        })
        .collect()
}

fn prompt_environment() -> Result<Option<String>, String> {
    let configuration = Configuration::load()?;
    let names = configuration.environments.keys().collect::<Vec<_>>();
    println!("\nEnvironments");
    for (index, name) in names.iter().enumerate() {
        let marker = if name.as_str() == configuration.current_environment {
            " (current)"
        } else {
            ""
        };
        println!("  {}  {}{marker}", index + 1, name);
    }
    let answer = prompt("Choose an environment [current]: ")?;
    if answer.is_empty() {
        return Ok(None);
    }
    if let Ok(index) = answer.parse::<usize>() {
        return names
            .get(
                index
                    .checked_sub(1)
                    .ok_or_else(|| format!("environment number {index} is out of range"))?,
            )
            .map(|name| Some((*name).clone()))
            .ok_or_else(|| format!("environment number {index} is out of range"));
    }
    Configuration::validate_environment_name(&answer)?;
    Ok(Some(answer))
}

fn production_confirmation(action: Action, environment: &str) -> Result<bool, String> {
    if environment != "production" || !action.can_test() {
        return Ok(false);
    }
    let answer = prompt("Type production to allow test commands to receive production settings: ")?;
    if answer == "production" {
        Ok(true)
    } else {
        Err("production workspace run cancelled".into())
    }
}

fn prompt(message: &str) -> Result<String, String> {
    print!("{message}");
    io::stdout()
        .flush()
        .map_err(|error| format!("could not write prompt: {error}"))?;
    let mut answer = String::new();
    let read = io::stdin()
        .read_line(&mut answer)
        .map_err(|error| format!("could not read prompt: {error}"))?;
    if read == 0 {
        return Err("input closed while waiting for a selection".into());
    }
    Ok(answer.trim().to_owned())
}

fn print_banner(color: bool) {
    println!("{}", paint("1;36", "LayerX Workspace", color));
    println!("Install dependencies, build modules, and run their real test gates.\n");
}

fn print_module_table<'a>(modules: impl Iterator<Item = &'a Module>) {
    println!("{:<12} {:<24} DESCRIPTION", "MODULE", "SURFACE");
    println!("{}", "─".repeat(82));
    for module in modules {
        println!(
            "{:<12} {:<24} {}",
            module.id, module.name, module.description
        );
    }
}

fn print_doctor(modules: &[&Module], checks: &[(&str, bool)], environment: &EnvironmentContext) {
    let color = color_enabled();
    print_banner(color);
    println!(
        "Host         {} / {}\nEnvironment  {}  {}  network {}\n",
        env::consts::OS,
        env::consts::ARCH,
        environment.name,
        environment.endpoint,
        environment.network_id
    );
    print_module_table(modules.iter().copied());
    println!("\nTOOLS");
    for (tool, found) in checks {
        if *found {
            println!("  {}  {tool}", paint("32", "✓", color));
        } else {
            println!(
                "  {}  {tool} — {}",
                paint("31", "✗", color),
                install_hint(tool)
            );
        }
    }
}

fn print_plan(
    action: Action,
    plan: &[PlannedStep<'_>],
    environment: &EnvironmentContext,
    dry_run: bool,
) {
    let color = color_enabled();
    print_banner(color);
    println!("Action       {}", action.label());
    println!(
        "Environment  {}  {}  network {}",
        environment.name, environment.endpoint, environment.network_id
    );
    println!(
        "Mode         {}",
        if dry_run { "dry run" } else { "execute" }
    );
    println!("Steps        {}\n", plan.len());
}

fn print_step(
    index: usize,
    total: usize,
    planned: &PlannedStep<'_>,
    command: &str,
    missing: &[&str],
) {
    let color = color_enabled();
    let heading = format!(
        "[{index}/{total}] {} {:<10} {}",
        planned.phase.label(),
        planned.module.id,
        planned.step.label
    );
    println!("{}", paint("1;34", &heading, color));
    println!("  $ {command}");
    if !missing.is_empty() {
        println!(
            "  {} missing {}",
            paint("31", "BLOCKED", color),
            missing.join(", ")
        );
    }
}

fn print_summary(results: &[StepResult], failed: usize) {
    let color = color_enabled();
    println!("\nSUMMARY");
    for result in results {
        let (code, glyph) = match result.status {
            "passed" => ("32", "✓"),
            "planned" => ("36", "○"),
            "not-needed" => ("90", "–"),
            _ => ("31", "✗"),
        };
        println!(
            "  {}  {:<10} {:<8} {:<12} {} ms",
            paint(code, glyph, color),
            result.module,
            result.phase,
            result.status,
            result.duration_ms
        );
    }
    if failed == 0 {
        println!("\n{}", paint("1;32", "Workspace run complete", color));
    } else {
        println!(
            "\n{}",
            paint("1;31", &format!("{failed} steps need attention"), color)
        );
    }
}

fn display_command(step: &Step) -> String {
    let Some(program) = step.program else {
        return "(nothing to install)".into();
    };
    std::iter::once(program)
        .chain(step.args.iter().copied())
        .map(display_argument)
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_argument(argument: &str) -> String {
    if argument
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_=./:".contains(character))
    {
        argument.to_owned()
    } else {
        format!("{argument:?}")
    }
}

fn color_enabled() -> bool {
    io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none()
}

fn paint(code: &str, text: &str, enabled: bool) -> String {
    if enabled {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_owned()
    }
}

fn module_json(module: &Module) -> Value {
    json!({
        "id": module.id,
        "name": module.name,
        "description": module.description,
        "tools": module.tools,
        "install_steps": module.install.len(),
        "build_steps": module.build.len(),
        "test_steps": module.test.len(),
    })
}

fn environment_json(environment: &EnvironmentContext) -> Value {
    json!({
        "name": environment.name,
        "endpoint": environment.endpoint,
        "network_id": environment.network_id,
    })
}

fn result_json(result: &StepResult) -> Value {
    json!({
        "module": result.module,
        "phase": result.phase,
        "label": result.label,
        "command": result.command,
        "status": result.status,
        "duration_ms": result.duration_ms,
        "missing_tools": result.missing_tools,
    })
}

#[cfg(test)]
mod tests {
    use super::{display_argument, select_modules, MODULES};

    #[test]
    fn all_selection_covers_every_module() {
        let selected = select_modules(&[], true, false);
        match selected {
            Ok(selected) => assert_eq!(selected.len(), MODULES.len()),
            Err(error) => panic!("all modules should resolve: {error}"),
        }
    }

    #[test]
    fn explicit_selection_is_ordered_and_deduplicated() {
        let requested = vec!["human".to_string(), "core".to_string(), "human".to_string()];
        let selected = select_modules(&requested, false, false);
        match selected {
            Ok(selected) => {
                let ids = selected.iter().map(|module| module.id).collect::<Vec<_>>();
                assert_eq!(ids, ["human", "core"]);
            }
            Err(error) => panic!("selected modules should resolve: {error}"),
        }
    }

    #[test]
    fn unknown_module_is_rejected() {
        let result = select_modules(&["unknown".to_string()], false, false);
        assert!(result.is_err());
    }

    #[test]
    fn display_quotes_only_unsafe_arguments() {
        assert_eq!(display_argument("agent/Cargo.toml"), "agent/Cargo.toml");
        assert_eq!(display_argument("two words"), "\"two words\"");
    }
}
