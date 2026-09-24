---
name: create-pr
description: Create a well-structured pull request with proper description, labels, and reviewers
disable-model-invocation: true
---
# Create PR

## Prerequisites: GitHub CLI

This workflow uses `gh` to check for existing PRs, fetch issue details, and create the PR. **Follow the check-github-cli skill** first (ensure `gh auth status` succeeds; if not, guide the user to install/configure `gh` and do not proceed). The account needs read access to the repo and write access to create PRs and manage labels.

## Overview

Create a well-structured pull request with proper description, labels, and reviewers.

## Steps

_Note:_ In the command output, format all URLs as clickable Markdown links: `[Link Text](URL)`.

### 1. Check that a PR is not already open for this branch

- Get the current branch: `git branch --show-current`.
- List open PRs for that branch: `gh pr list --head <BRANCH> --state open`.
- If a PR exists, do **only** the PR summary (see step 3: write the description content as if for the PR, but do not create or update the PR). Skip steps 2, 3 (create/update), and 4. Tell the user the PR is already open and link to it.

### 2. Prepare the branch

- Ensure all changes are committed:
  - `git status` to see uncommitted changes.
  - If there are changes: `git add` (as appropriate), then `git commit -m "..."` with a clear message.
- Push the branch: `git push -u origin <BRANCH>` (or `git push` if upstream is already set).
- Verify the branch is up to date with main:
  - `git fetch origin main` then compare: e.g. `git rev-list --left-right --count origin/main...HEAD` (expect `0	N` for “main has nothing we don’t have”) or merge/rebase if the user wants: `git merge origin/main`.
- Resolve any merge conflicts before continuing.

### 3. Write the PR description

- Use the template from `./docs/PULL_REQUEST_TEMPLATE.md` as the base. Read it and fill in each section.
- **Issue number from branch name:** If the branch name matches the pattern `issue-\d+` (e.g. `issue-123`), extract the number (e.g. `git branch --show-current | grep -oE 'issue-[0-9]+' | grep -oE '[0-9]+'`), then:
  - Fetch the issue: `gh issue view <ISSUE_NUMBER>`.
  - At the very beginning of the **"Description and Related Issue(s)"** section, include: `Resolves #<ISSUE_NUMBER>`.
- **Summary of changes:** Summarize the changes clearly and concisely in no more than two paragraphs. Use bullet points if needed. Be precise; keep the description short.
- **Environment variable changes:** If any env vars were added, changed, or documented:
  - Compare or read `./docs/ENVS.md` (and the validator/ENVS docs if relevant) to list what changed.
  - Add a separate section listing each variable change and the **purpose** of each (why it was added/changed).
    - **Bad:** "Added `NEXT_PUBLIC_VIEWS_TX_GROUPED_FEES` environment variable to the documentation."
    - **Good:** "Added `NEXT_PUBLIC_VIEWS_TX_GROUPED_FEES` to group transaction fees into one section on the transaction page."
    - **Good:** "Extended possible values for `NEXT_PUBLIC_VIEWS_TX_ADDITIONAL_FIELDS` with set_max_gas_limit to display the maximum gas price set by the transaction sender."
    - **Good:** "Introduced a new option, `"fee reception"`, for the `NEXT_PUBLIC_NETWORK_VERIFICATION_TYPE` variable."
- **Checklist:** Keep the "Checklist for PR Author" section from the template and check the items that apply (e.g. tested locally, tests added, ENVS/docs/validator updated if env vars changed).
- When the description is ready, **ask the user for confirmation or changes** before creating the PR (step 4).

### 4. Create the PR and set labels

- Create the pull request with a descriptive title. Use draft mode if the user asked for it:
  - `gh pr create --title "Your descriptive title" --body-file /path/to/body.md` (and add `--draft` if draft).
  - Or paste the body inline if preferred; ensure the filled template is used.
- Add labels:
  - If anything was added or changed in `./docs/ENVS.md`, add the **"ENVs"** label: `gh pr edit <PR_NUMBER> --add-label "ENVs"` (or add labels during `gh pr create` if your `gh` version supports it).
  - If `package.json` has changed (check with `git diff origin/main -- package.json`), add the **"dependencies"** label.
  - If the branch name contained an issue number (pattern `issue-\d+`), copy all labels from that issue onto the PR: get labels with `gh issue view <ISSUE_NUMBER> --json labels -q '.labels[].name'`, then add each: `gh pr edit <PR_NUMBER> --add-label "<LABEL>"`.
- Include a clickable link to the created PR in the command output (e.g. from `gh pr view --web` or the URL printed by `gh pr create`).
