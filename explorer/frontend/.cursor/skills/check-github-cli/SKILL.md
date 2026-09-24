---
name: check-github-cli
description: Ensure GitHub CLI (gh) is installed and authenticated before running gh commands; guide the user to configure it if not
---
# Check GitHub CLI Availability

Use this skill whenever a workflow or command requires the GitHub CLI (`gh`). Before running any `gh` commands:

1. **Check installation and auth**: Run `gh auth status`. If it reports "not logged in" or the command is not found:
   - **If `gh` is not installed**: Ask the user to install it (e.g. `brew install gh` on macOS, or see [GitHub CLI](https://cli.github.com/)) and then authenticate.
   - **If `gh` is installed but not configured**: Ask the user to configure it themselves. Provide these instructions to help them:
     - Run `gh auth login` and follow the prompts (choose HTTPS or SSH, authenticate via browser or token).
     - Ensure the account has the permissions needed for the operation (e.g. read access to the repository, or read/write if the task requires it).
   - Do not attempt to authenticate on the user's behalf; only guide them with the commands and link to [GitHub CLI authentication docs](https://cli.github.com/manual/gh_auth_login).
2. **If `gh auth status` succeeds**: Proceed with the steps that require `gh`.

Other skills that depend on `gh` (e.g. get-checks-status) should instruct the agent to ensure GitHub CLI is ready first by following this skill.
