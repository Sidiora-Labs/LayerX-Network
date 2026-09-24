---
name: deploy-demo
description: Create a GitHub deployment for the current branch using the deploy-review workflow
disable-model-invocation: true
---
# Deploy demo

## Prerequisites: GitHub CLI

This workflow uses `gh` to trigger and monitor the deployment workflow. **Follow the check-github-cli skill** first (ensure `gh auth status` succeeds; if not, guide the user to install/configure `gh` and do not proceed). The account needs permission to trigger workflows and read the repository.

## Overview

Create a GitHub deployment for the current branch using `.github/workflows/deploy-review.yml` (workflow name: "Deploy review environment").

## Steps

_Note:_ In the command output, format all URLs as clickable Markdown links: `[Link Text](URL)`.

### 1. Ensure all changes are pushed to remote

- Check status: `git status`. If there are uncommitted changes, ask the user to commit and push, or run `git add` / `git commit` and then push.
- Push the current branch: `git push origin $(git branch --show-current)` (or `git push` if upstream is set). Abort if push fails.

### 2. Check for an already running deployment for the current branch

- Get current branch: `git branch --show-current`.
- List runs for the deploy-review workflow on this branch that are in progress or queued:
  - `gh run list --workflow=deploy-review.yml --branch <BRANCH> --status in_progress`
  - `gh run list --workflow=deploy-review.yml --branch <BRANCH> --status queued`
- If any run is returned, **abort the command** and tell the user a deployment is already running for this branch (include a link to the run).

### 3. Resolve the "envs_preset" workflow input (optional)

- **Get valid options from the workflow file (single source of truth):** Read `.github/workflows/deploy-review.yml` and extract the list of allowed values from `on.workflow_dispatch.inputs.envs_preset.options` (the YAML array under that key). Do not hardcode or copy the preset list into this skill — always read it from the file.
- Infer from the user’s prompt which preset they want. Examples:
  - "deploy a demo for base" or "deploy with base preset" → preset `base`
  - "spin up a demo using main preset" → preset `main`
- **If the user did not mention any preset:** skip passing `envs_preset` (the workflow uses its default).
- **Validate the chosen preset:** Check that the user’s chosen preset is one of the values in the options list you extracted from the workflow file. If it is not in that list, notify the user (you can show the valid options from the file) and **abort the command**.

### 4. Trigger the workflow and capture the run ID

- Run the workflow from the current branch (use the branch name from step 1 or 2):
  - Without preset: `gh workflow run deploy-review.yml --ref <BRANCH>`
  - With preset: `gh workflow run deploy-review.yml --ref <BRANCH> -f envs_preset=<PRESET>`
- The CLI does not print the new run ID. To get it: wait a few seconds, then list the latest run for this workflow and branch: `gh run list --workflow=deploy-review.yml --branch <BRANCH> --limit 1`. Use that run’s ID for the next steps.

### 5. Monitor the workflow until it completes

- Watch the run: `gh run watch <RUN_ID>` (polls until completion).
- Track the outcome: success, failure, or cancelled (from the watch output or from `gh run view <RUN_ID>`).
- When completed, get outputs and deployment URL: `gh run view <RUN_ID>` and, if available, `gh run view <RUN_ID> --log` or job outputs. The deployment URL may appear in the workflow summary or in job outputs; extract it and include it in the notification.

### 6. Send system notifications with results

- **When the workflow is triggered:** Notify the user that the deployment has been started (include a link to the run).
- **When the workflow completes:** Notify with the final status (success, failure, or cancelled), the deployment URL if available, and any other relevant outputs.
- **On failure:** Notify with the error and details (e.g. failed job/step from `gh run view <RUN_ID>` or `gh run view <RUN_ID> --log-failed`).
