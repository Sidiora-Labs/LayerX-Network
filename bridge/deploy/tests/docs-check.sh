#!/usr/bin/env bash
# Offline check of the Paxeer X Network bridge documentation: every Markdown page
# under bridge/. Every link must be https://paxeer.app or a path under
# https://github.com/Sidiora-Labs/Paxeer-X-Network; every repository path a page
# names must exist; no page may carry a date, a hostname, an IP address, a
# credential, or an agent, model or working-branch name. The operator runbook and
# the nine per-chain pages must also exist, name the product Paxeer X Network,
# name their pair, chain id and environment variables as the chain configuration
# declares them, and the hyperevm and Solana pages must carry what only those
# chains need; the runbook's submission section must name both governance
# proposals and the node's submit command with the bridge subcommand, whose Use
# line it reads from modules/layerxbridge/client/cli/tx.go, and each proposal
# file as its argument, and must not claim the bodies have no path to the chain
# or that no command submits the proposals. Section 6 and the Sidiora section
# must read the Sidiora ordering from the proposal itself - 05-proposal-sidiora-cap.json
# registers the pair with MsgRegisterSidioraPair ahead of Sidiora's cap - and
# must not send the operator to an upgrade handler or to a read-back before that
# proposal. The check then mutates a copy of the
# pages one way at a time and requires each mutation to be refused, so every
# assertion is known to bite.
# No network is involved.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)

fail() {
    printf 'docs-check: error: %s\n' "$*" >&2
    exit 1
}

for tool in python3 jq; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required and is not on the PATH"
done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

CHECKER="$WORK/check_docs.py"
cat > "$CHECKER" << 'PY'
import glob
import json
import os
import re
import sys

docs_root, repo_root = sys.argv[1], sys.argv[2]

EXCLUDED_DIRS = {"lib", "target", "build", "node_modules", ".git"}

EVM_CHAINS = {
    "ethereum": ("ETH", 1),
    "base": ("ETH", 8453),
    "arbitrum": ("ETH", 42161),
    "optimism": ("ETH", 10),
    "bnb": ("BNB", 56),
    "polygon": ("POL", 137),
    "avalanche": ("AVAX", 43114),
    "hyperevm": ("HYPE", 999),
}
SOLANA_CHAIN_ID = 91600046870081
SIDIORA_MINT = "5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump"
SIDIORA_ASSET_ID = "0x21f7b20a555199fa73A238B1a91FD0f549068fEe"
RUNBOOK = "bridge/README.md"
PROPOSAL_FILES = ("04-proposal-open-chain.json", "05-proposal-sidiora-cap.json")
SUBMIT_COMMAND = "paxd tx gov submit-proposal"
PROPOSAL_CLI = "modules/layerxbridge/client/cli/tx.go"
SIDIORA_PAIR_MSG = "MsgRegisterSidioraPair"
SIDIORA_ORDERING_RULE = "The ordering rule is carried by the proposal itself."
STALE_SIDIORA_CLAIMS = [
    ("that no generated body or proposal registers the Sidiora pair", re.compile(
        r"no\s+generated\s+(?:body|proposal|message)\s+registers\s+the\s+pair", re.I)),
    ("that an upgrade handler registers the Sidiora pair for the bridge", re.compile(
        r"production\s+caller\s+is\s+the\s+handler\s+of\s+the\s+`?v\d+\.\d+`?\s+upgrade", re.I)),
    ("that the Sidiora proposal waits for a read-back of the pair", re.compile(
        r"only\s+after\s+the\s+usid\s+pair\s+reads\s+back"
        r"|Submit\s+`?05-proposal-sidiora-cap\.json`?\s+only\s+when"
        r"|Before\s+`?05-proposal-sidiora-cap\.json`?,\s+read\s+the\s+pair\s+back", re.I)),
]
STALE_SUBMISSION_CLAIMS = [
    ("that the module registers no message service", re.compile(r"registers\s+no\s+(?:message|Msg)\s+service", re.I)),
    ("that no command carries the bodies", re.compile(
        r"no\s+(?:transaction\s+)?command\s+(?:that\s+)?(?:broadcasts|carries|submits)\s+(?:them|the\s+(?:message\s+)?bodies)\b"
        r"|carries\s+no\s+command\s+that\s+broadcasts", re.I)),
    ("that no command the node exposes submits the proposals", re.compile(
        r"no\s+command\s+the\s+node\s+exposes(?:\s+today)?\s+submits"
        r"|builds\s+only\s+a\s+`?Text`?\s+proposal", re.I)),
]

ALLOWED_LINK = re.compile(
    r"^https://(?:paxeer\.app|github\.com/Sidiora-Labs/Paxeer-X-Network)(?:/[^\s]*)?$"
)
ALLOWED_HOSTS = {"paxeer.app"}

INLINE_LINK = re.compile(r"!?\[[^\]]*\]\(\s*<?([^)\s>]*)>?(?:\s+\"[^\"]*\")?\s*\)")
REFERENCE_LINK = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s|$)", re.M)
AUTOLINK = re.compile(r"<([A-Za-z][A-Za-z0-9+.-]*:[^>\s]+)>")
BARE_URL = re.compile(r"\b(?:[A-Za-z][A-Za-z0-9+.-]*://|www\.)[^\s<>()`|\"']+")
HTML_LINK = re.compile(r"\b(?:href|src)\s*=\s*[\"']([^\"']+)[\"']", re.I)

PATH_TOKEN = re.compile(
    r"(?<![\w./@:-])(?:\./)?((?:bridge|interop|modules|precompiles|node|sdk|spec|docs|contracts|tools|\.github)"
    r"/[A-Za-z0-9_.<>*/-]*)"
)
GO_QUALIFIED = re.compile(r"^(.+)\.([A-Z][A-Za-z0-9_]*)$")

MONTHS = (
    r"(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|June?|July?|Aug(?:ust)?"
    r"|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)"
)
DATE_PATTERNS = [
    re.compile(r"\b(?:19|20)\d{2}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])\b"),
    re.compile(r"\b(?:19|20)\d{2}/(?:0?[1-9]|1[0-2])/(?:0?[1-9]|[12]\d|3[01])\b"),
    re.compile(r"\b(?:0?[1-9]|[12]\d|3[01])[/.](?:0?[1-9]|1[0-2])[/.](?:19|20)\d{2}\b"),
    re.compile(r"\b" + MONTHS + r"\.? (?:0?[1-9]|[12]\d|3[01])(?:st|nd|rd|th)?\b"),
    re.compile(r"\b(?:0?[1-9]|[12]\d|3[01])(?:st|nd|rd|th)? " + MONTHS + r"\b"),
    re.compile(r"\b" + MONTHS + r",? (?:19|20)\d{2}\b"),
]
IPV4 = re.compile(r"(?<![\w.])(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)(?!\w|\.\d)")
IPV6 = re.compile(r"(?<![\w:])(?:[0-9A-Fa-f]{1,4}:){3,7}[0-9A-Fa-f]{1,4}(?![\w:])|(?<![\w:])(?:[0-9A-Fa-f]{1,4}:){1,6}:[0-9A-Fa-f]{0,4}(?![\w:])")
HOSTNAME = re.compile(
    r"(?<![\w.@/-])((?:[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?\.)+"
    r"(?:com|net|org|io|app|dev|xyz|co|cloud|internal|local|lan|corp|home|ai|gg|me|network|site|tech|info|biz|us|eu|de|uk))"
    r"(?![\w-])",
    re.I,
)
LOCALHOST = re.compile(r"\blocalhost\b", re.I)
CREDENTIAL_PATTERNS = [
    ("a PEM private key", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("a cloud access key", re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
    ("a GitHub token", re.compile(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b")),
    ("a Slack token", re.compile(r"\bxox[abprs]-[A-Za-z0-9-]{10,}")),
    ("an API secret key", re.compile(r"\bsk-[A-Za-z0-9_-]{20,}")),
    ("a bearer token", re.compile(r"\bBearer\s+[A-Za-z0-9._~+/=-]{16,}")),
    ("credentials in a URL", re.compile(r"[A-Za-z][A-Za-z0-9+.-]*://[^/\s:@]+:[^/\s@]+@")),
    ("an assigned secret", re.compile(
        r"(?i)\b(?:api[_-]?key|secret|password|passwd|token|private[_ -]?key|mnemonic|seed[_ -]?phrase)\b"
        r"\s*[:=]\s*[\"']?[A-Za-z0-9+/_.=-]{12,}")),
    ("a private key in hex", re.compile(r"(?i)\b(?:private[_ -]?key|secret[_ -]?key|secret)\b[^\n]{0,40}?\b(?:0x)?[0-9a-f]{64}\b")),
    ("a keypair byte array", re.compile(r"\[\s*(?:\d{1,3}\s*,\s*){31,}\d{1,3}\s*\]")),
]
INTERNAL_NAME_PATTERNS = [
    ("a working-branch name", re.compile(r"(?<![\w/-])(?:wave|lane|feature)/[a-z0-9][a-z0-9._/-]*")),
    ("a fleet role id", re.compile(r"\bpx\d+-[a-z]+\d*\b")),
    ("an agent or model name", re.compile(r"\b(?:Claude|Codex|Opus|Sonnet|Haiku|Grok|Fable|GPT-?\d|gpt-\d)\b")),
]

problems = []


def problem(page, line, message):
    problems.append("%s:%d: %s" % (page, line, message))


def line_of(text, offset):
    return text.count("\n", 0, offset) + 1


def pages():
    found = []
    base = os.path.join(docs_root, "bridge")
    for directory, subdirectories, files in os.walk(base):
        subdirectories[:] = sorted(d for d in subdirectories if d not in EXCLUDED_DIRS)
        for name in sorted(files):
            if name.endswith(".md"):
                found.append(os.path.relpath(os.path.join(directory, name), docs_root))
    return sorted(found)


def path_exists(token):
    token = token.rstrip(".,;:")
    token = token.rstrip("/")
    if not token:
        return True
    pattern = re.sub(r"<[^>]*>", "*", token)
    if glob.glob(os.path.join(repo_root, pattern)):
        return True
    qualified = GO_QUALIFIED.match(pattern)
    if qualified and os.path.isdir(os.path.join(repo_root, qualified.group(1))):
        return True
    return False


def links(text):
    for regex in (INLINE_LINK, REFERENCE_LINK, AUTOLINK, HTML_LINK):
        for match in regex.finditer(text):
            yield match.start(1), match.group(1)
    for match in BARE_URL.finditer(text):
        yield match.start(), match.group(0).rstrip(".,;:!?")


def check_page(page):
    with open(os.path.join(docs_root, page), encoding="utf-8") as handle:
        text = handle.read()
    stats = {"links": 0, "paths": 0}
    seen_links = set()
    for offset, target in links(text):
        if (offset, target) in seen_links:
            continue
        seen_links.add((offset, target))
        stats["links"] += 1
        if not ALLOWED_LINK.match(target):
            problem(page, line_of(text, offset),
                    "the link %s is neither https://paxeer.app nor a path under "
                    "https://github.com/Sidiora-Labs/Paxeer-X-Network" % target)
    for match in PATH_TOKEN.finditer(text):
        stats["paths"] += 1
        if not path_exists(match.group(1)):
            problem(page, line_of(text, match.start(1)),
                    "names the repository path %s, which does not exist" % match.group(1))
    for regex in DATE_PATTERNS:
        for match in regex.finditer(text):
            problem(page, line_of(text, match.start()), "carries the date %r" % match.group(0))
    for regex, label in ((IPV4, "IPv4"), (IPV6, "IPv6")):
        for match in regex.finditer(text):
            problem(page, line_of(text, match.start()), "carries the %s address %s" % (label, match.group(0)))
    without_links = BARE_URL.sub(lambda m: " " * len(m.group(0)), text)
    for match in HOSTNAME.finditer(without_links):
        if match.group(1).lower() not in ALLOWED_HOSTS:
            problem(page, line_of(text, match.start(1)), "carries the hostname %s" % match.group(1))
    for match in LOCALHOST.finditer(without_links):
        problem(page, line_of(text, match.start()), "carries the hostname %s" % match.group(0))
    for label, regex in CREDENTIAL_PATTERNS:
        for match in regex.finditer(text):
            problem(page, line_of(text, match.start()), "carries %s" % label)
    for label, regex in INTERNAL_NAME_PATTERNS:
        for match in regex.finditer(text):
            problem(page, line_of(text, match.start()), "carries %s: %s" % (label, match.group(0)))
    return text, stats


def require(page, text, needle, why):
    if needle not in text:
        problem(page, 1, "does not carry %r: %s" % (needle, why))


def load_config(relative):
    with open(os.path.join(repo_root, relative), encoding="utf-8") as handle:
        return json.load(handle)


def proposal_use_line():
    try:
        with open(os.path.join(repo_root, PROPOSAL_CLI), encoding="utf-8") as handle:
            source = handle.read()
    except OSError:
        return None
    name = re.search(r'ProposalCommandName\s*=\s*"([a-z0-9-]+)"', source)
    use = re.search(r'Use:\s*ProposalCommandName\s*\+\s*"( [^"]*)"', source)
    if not name or not use:
        return None
    return name.group(1) + use.group(1)


def check_product_page(page, text):
    require(page, text, "Paxeer X Network", "every page names the product Paxeer X Network")
    for match in re.finditer(r"\bLayerX\b(?!Bridge|\w)", text):
        problem(page, line_of(text, match.start()),
                "names LayerX as a product; LayerX names only the kernel entities that already carry it")


def check_chain_page(page, text, config_path, chain, symbol, chain_id):
    config = load_config(config_path)
    if config.get("chain") != chain or config.get("chain_id") != chain_id:
        problem(page, 1, "%s does not declare chain %s with id %d" % (config_path, chain, chain_id))
    require(page, text, "PAX against %s" % symbol, "the page names its pair in plain words")
    require(page, text, "`%d`" % chain_id, "the page names its chain id")
    require(page, text, config_path, "the page names its configuration")
    for field, variable in sorted(config.get("environment", {}).items()):
        require(page, text, "`%s`" % variable, "the configuration names %s in environment.%s" % (variable, field))
    for asset in config.get("assets", []):
        require(page, text, "`%s`" % asset["address"], "the configuration registers %s" % asset["symbol"])
        require(page, text, "`%s`" % asset["asset_id"], "the configuration registers %s" % asset["symbol"])
        for cap in ("per_tx_cap", "total_cap"):
            require(page, text, "`%s`" % asset[cap], "the configuration caps %s at %s" % (asset["symbol"], asset[cap]))


all_pages = pages()
required = [RUNBOOK] + ["bridge/evm/chains/%s/README.md" % chain for chain in EVM_CHAINS]
required.append("bridge/solana/chains/solana/README.md")
for page in required:
    if page not in all_pages:
        problems.append("%s: the page is missing" % page)

totals = {"links": 0, "paths": 0}
texts = {}
for page in all_pages:
    text, stats = check_page(page)
    texts[page] = text
    for key in totals:
        totals[key] += stats[key]

for page in required:
    if page not in texts:
        continue
    check_product_page(page, texts[page])

if RUNBOOK in texts:
    runbook = texts[RUNBOOK]
    steps = [
        "bash bridge/evm/bootstrap-libs.sh",
        "bash bridge/deploy/deploy-evm-chain.sh",
        "bash bridge/deploy/verify-evm-chain.sh",
        "bash bridge/deploy/deploy-solana-program.sh",
        "go run ./bridge/deploy/proposals/cmd/paxeer-bridge-proposals",
    ]
    positions = []
    for step in steps:
        position = runbook.find(step)
        if position < 0:
            problem(RUNBOOK, 1, "does not carry the step %r" % step)
        positions.append(position)
    if all(p >= 0 for p in positions) and positions != sorted(positions):
        problem(RUNBOOK, 1, "does not carry the steps in the order bootstrap, deploy, verify, Solana, proposals")
    for heading in ("### 6. Submit the proposals", "### 7. Read the deployment back"):
        require(RUNBOOK, runbook, heading, "the runbook orders submission and the read-back after the proposals")
    for variable in ("PAXEER_BRIDGE_DEPLOYMENT_RECORD", "PAXEER_BRIDGE_EVM_CHAINS_ROOT"):
        require(RUNBOOK, runbook, "`%s`" % variable, "the runbook names every variable its steps need")
    for chain, (symbol, chain_id) in EVM_CHAINS.items():
        require(RUNBOOK, runbook, "bridge/evm/chains/%s/README.md" % chain, "the runbook points at every chain page")
    require(RUNBOOK, runbook, "bridge/solana/chains/solana/README.md", "the runbook points at every chain page")
    sidiora = runbook.find("## The Sidiora pair on Solana")
    if sidiora < 0:
        problem(RUNBOOK, 1, "does not carry the section on the Sidiora pair")
    else:
        section = runbook[sidiora:]
        for needle in ("getCap(uint64,address)", "91600046870081 " + SIDIORA_ASSET_ID, "/usid"):
            require(RUNBOOK, section, needle, "the Sidiora section says what to read back once the Sidiora proposal has passed")
        require(RUNBOOK, section, "`%s` registers the pair against `usid`" % SIDIORA_PAIR_MSG,
                "the Sidiora section names the message that registers the pair")
        require(RUNBOOK, section, SIDIORA_ORDERING_RULE, "the Sidiora section reads the ordering rule from the proposal itself")
        require(RUNBOOK, section, "`%s` first and Sidiora's `MsgSetCap` second" % SIDIORA_PAIR_MSG,
                "the Sidiora section says the proposal registers the pair ahead of the cap")
    for label, regex in STALE_SIDIORA_CLAIMS:
        for match in regex.finditer(runbook):
            problem(RUNBOOK, line_of(runbook, match.start()),
                    "still claims %s; 05-proposal-sidiora-cap.json registers the pair ahead of its cap" % label)
    submit = runbook.find("### 6. Submit the proposals")
    readback = runbook.find("### 7. Read the deployment back")
    if submit >= 0 and readback > submit:
        section = runbook[submit:readback]
        for name in PROPOSAL_FILES:
            require(RUNBOOK, section, "`%s`" % name, "section 6 names every proposal the generator writes through -proposals")
        require(RUNBOOK, section, "`%s`" % SUBMIT_COMMAND, "section 6 names the node's governance submit command")
        require(RUNBOOK, section, "carries `%s` ahead of Sidiora's `MsgSetCap`" % SIDIORA_PAIR_MSG,
                "section 6 says the Sidiora proposal registers the pair ahead of the cap")
        use_line = proposal_use_line()
        if use_line is None:
            problem(RUNBOOK, 1, "%s carries no ProposalCommandName and Use line for the bridge subcommand" % PROPOSAL_CLI)
        else:
            name = use_line.split(" ")[0]
            require(RUNBOOK, section, "`%s`" % use_line, "section 6 names the bridge subcommand exactly as its Use line reads")
            for file_name in PROPOSAL_FILES:
                require(RUNBOOK, section, "%s %s <proposals directory>/%s --deposit <coins> --from <key>" % (SUBMIT_COMMAND, name, file_name),
                        "section 6 shows the submit command with the generated file as its argument")
    require(RUNBOOK, runbook, "-proposals <proposals directory>", "section 5 runs the generator with its -proposals output")
    for label, regex in STALE_SUBMISSION_CLAIMS:
        for match in regex.finditer(runbook):
            problem(RUNBOOK, line_of(runbook, match.start()),
                    "still claims %s; the proposals carry the bodies through governance" % label)

for chain, (symbol, chain_id) in EVM_CHAINS.items():
    page = "bridge/evm/chains/%s/README.md" % chain
    if page in texts:
        check_chain_page(page, texts[page], "bridge/evm/chains/%s/config.json" % chain, chain, symbol, chain_id)

hyperevm = "bridge/evm/chains/hyperevm/README.md"
if hyperevm in texts:
    require(hyperevm, texts[hyperevm], "big blocks", "HyperEVM needs the deploying account switched to big blocks")
    require(hyperevm, texts[hyperevm], "`acknowledged`", "the deploy script refuses HyperEVM until the acknowledgement is recorded")

solana = "bridge/solana/chains/solana/README.md"
if solana in texts:
    text = texts[solana]
    check_chain_page(solana, text, "bridge/solana/chains/solana/config.json", "solana", "SOL", SOLANA_CHAIN_ID)
    require(solana, text, SIDIORA_MINT, "the Solana page names Sidiora's mint")
    require(solana, text, SIDIORA_ASSET_ID, "the Solana page names Sidiora's asset id")
    require(solana, text, "foreign home", "the Solana page says Solana is Sidiora's foreign home")
    require(solana, text, "EnsureSidioraDenom", "the Solana page says why the chain fixes Sidiora's asset id")

if problems:
    for line in problems:
        print("docs-check: " + line, file=sys.stderr)
    sys.exit(1)
print("docs-check: %d pages, %d links and %d repository paths checked" % (len(all_pages), totals["links"], totals["paths"]))
PY

python3 "$CHECKER" "$REPO_ROOT" "$REPO_ROOT" || fail "the bridge documentation does not pass"

# Each mutation below breaks one assertion on a copy of the pages and must be
# refused; a mutation that passes means the assertion does not bite.
copy_pages() {
    local destination=$1
    (cd "$REPO_ROOT" && find bridge -name '*.md' -not -path '*/lib/*' -not -path '*/target/*' -not -path '*/build/*' -print0) \
        | while IFS= read -r -d '' page; do
            mkdir -p "$destination/$(dirname "$page")"
            cp "$REPO_ROOT/$page" "$destination/$page"
        done
}

mutate() {
    local label=$1 page=$2 text=$3
    local root="$WORK/mutation"
    rm -rf "$root"
    mkdir -p "$root"
    copy_pages "$root"
    printf '\n%s\n' "$text" >> "$root/$page"
    if python3 "$CHECKER" "$root" "$REPO_ROOT" 2> "$WORK/mutation.log" > /dev/null; then
        fail "a page carrying $label was accepted"
    fi
    printf 'docs-check: refused %s\n' "$label"
}

remove_line() {
    local label=$1 page=$2 needle=$3
    local root="$WORK/mutation"
    rm -rf "$root"
    mkdir -p "$root"
    copy_pages "$root"
    grep -q -F -- "$needle" "$root/$page" || fail "$page does not carry $needle to remove"
    grep -v -F -- "$needle" "$root/$page" > "$root/$page.next" || true
    mv "$root/$page.next" "$root/$page"
    if python3 "$CHECKER" "$root" "$REPO_ROOT" 2> "$WORK/mutation.log" > /dev/null; then
        fail "a page without $label was accepted"
    fi
    printf 'docs-check: refused a page without %s\n' "$label"
}

mutate 'a link outside the allowlist' bridge/README.md 'See [the explorer](https://example.org/tx).'
mutate 'a relative link' bridge/README.md 'See [the vault](evm/src/PaxeerXVault.sol).'
mutate 'a lookalike of an allowed link' bridge/README.md 'See <https://github.com/Sidiora-Labs/Paxeer-X-Network-fork>.'
mutate 'a repository path that does not exist' bridge/evm/chains/base/README.md "Run \`bridge/deploy/no-such-script.sh\`."
mutate 'an ISO date' bridge/evm/chains/bnb/README.md 'Deployed on 2031-04-17.'
mutate 'a written date' bridge/solana/chains/solana/README.md 'Opened on March 3.'
mutate 'an IPv4 address' bridge/evm/chains/polygon/README.md 'The endpoint answers at 10.20.30.40.'
mutate 'a hostname' bridge/evm/chains/avalanche/README.md 'Point the script at rpc.example.internal first.'
mutate 'localhost' bridge/evm/chains/optimism/README.md 'Point the script at localhost first.'
mutate 'a PEM private key' bridge/ATTESTATION-SOLANA.md '-----BEGIN EC PRIVATE KEY-----'
mutate 'an assigned secret' bridge/evm/chains/arbitrum/README.md 'api_key = Zm9vYmFyYmF6cXV4MTIzNDU2'
mutate 'a keypair byte array' bridge/solana/chains/solana/README.md "[$(seq -s , 1 64)]"
mutate 'a working-branch name' bridge/evm/chains/ethereum/README.md 'Built from wave/bridge/2.5.'
mutate 'LayerX as a product name' bridge/evm/chains/hyperevm/README.md 'This is the LayerX bridge.'
remove_line 'the product name' bridge/evm/chains/ethereum/README.md 'Paxeer X Network'
remove_line 'its pair' bridge/evm/chains/bnb/README.md 'PAX against BNB'
remove_line 'its environment variables' bridge/evm/chains/base/README.md 'PAXEER_BRIDGE_BASE_EXPLORER_KEY'
remove_line 'the big-block requirement' bridge/evm/chains/hyperevm/README.md 'big blocks'
remove_line "Sidiora's asset id" bridge/solana/chains/solana/README.md '0x21f7b20a555199fa73A238B1a91FD0f549068fEe'
remove_line 'the checklist step' bridge/README.md '### 7. Read the deployment back'
remove_line 'the open-chain proposal' bridge/README.md '04-proposal-open-chain.json'
remove_line "the Sidiora cap proposal" bridge/README.md '05-proposal-sidiora-cap.json'
remove_line 'the governance submit command' bridge/README.md 'paxd tx gov submit-proposal'
remove_line 'the bridge subcommand' bridge/README.md 'through its bridge subcommand'
remove_line 'the open-chain submit command' bridge/README.md 'layerxbridge-proposal <proposals directory>/04-proposal-open-chain.json'
remove_line 'the Sidiora cap submit command' bridge/README.md 'layerxbridge-proposal <proposals directory>/05-proposal-sidiora-cap.json'
remove_line 'the -proposals output' bridge/README.md '-proposals <proposals directory>'
mutate 'the claim that the module registers no message service' bridge/README.md 'The bridge module registers no message service.'
mutate 'the claim that no command carries the bodies' bridge/README.md 'This repository carries no command that broadcasts them.'
mutate 'the claim that no command the node exposes submits the proposals' bridge/README.md 'No command the node exposes today submits 04-proposal-open-chain.json as its content.'
remove_line 'the ordering rule read from the proposal' bridge/README.md 'The ordering rule is carried by the proposal itself.'
remove_line 'the Sidiora pair message in section 6' bridge/README.md "carries \`MsgRegisterSidioraPair\` ahead of Sidiora's \`MsgSetCap\`"
remove_line 'the Sidiora pair message in the Sidiora section' bridge/README.md "- \`MsgRegisterSidioraPair\` registers the pair against"
mutate 'the claim that no generated body registers the Sidiora pair' bridge/README.md 'No generated body registers the pair against usid.'
mutate 'the claim that an upgrade handler registers the Sidiora pair' bridge/README.md "Its production caller is the handler of the \`v6.7\` upgrade in node/upgrades.go."
mutate 'the claim that the Sidiora proposal waits for a read-back' bridge/README.md "Submit \`05-proposal-sidiora-cap.json\` only when the denom it returns is the usid denom."

printf 'docs-check: the bridge documentation passes and every mutation is refused\n'
