#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Fetches labels and description (body) for a list of pull requests via the GitHub API,
 * with rate limiting to avoid hitting API limits. Outputs JSON to a file for use when
 * preparing release notes (mapping PRs to sections and building the ENV variables section).
 *
 * Prerequisites: GitHub CLI (gh) installed and authenticated.
 *
 * Usage:
 *   node fetch-release-prs.js <pr-number> [pr-number ...] [--out <path>]
 *   echo "2725 2726" | xargs node fetch-release-prs.js --out release-prs-data.json
 *
 * Output JSON shape (written to --out or stdout):
 *   { "prs": [ { "number", "title", "author", "url", "labels", "body" }, ... ] }
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const DELAY_MS = 400;

function getRepo() {
  try {
    return execSync('gh repo view --json nameWithOwner -q .nameWithOwner', {
      encoding: 'utf-8',
    }).trim();
  } catch {
    return 'blockscout/frontend';
  }
}

function fetchPr(ownerRepo, number) {
  const [ owner, repo ] = ownerRepo.split('/');
  const out = execSync(
    `gh api repos/${ owner }/${ repo }/issues/${ number } --jq '{number, title, user: .user.login, labels: [.labels[].name], body}'`,
    { encoding: 'utf-8', maxBuffer: 2 * 1024 * 1024 },
  );
  const data = JSON.parse(out);
  return {
    number: data.number,
    title: data.title,
    author: data.user,
    url: `https://github.com/${ ownerRepo }/pull/${ data.number }`,
    labels: data.labels || [],
    body: data.body || '',
  };
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  const args = process.argv.slice(2);
  let outPath = null;
  const prNumbers = [];

  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--out' && args[i + 1]) {
      outPath = args[i + 1];
      i += 1;
    } else if (/^\d+$/.test(args[i])) {
      prNumbers.push(Number(args[i], 10));
    }
  }

  if (prNumbers.length === 0) {
    console.error('Usage: node fetch-release-prs.js <pr-number> [pr-number ...] [--out <path>]');
    process.exit(1);
  }

  const ownerRepo = getRepo();
  const prs = [];

  for (let i = 0; i < prNumbers.length; i++) {
    const num = prNumbers[i];
    try {
      prs.push(fetchPr(ownerRepo, num));
      if (i < prNumbers.length - 1) {
        await sleep(DELAY_MS);
      }
    } catch (err) {
      console.error(`Failed to fetch PR #${ num }:`, err.message);
      process.exit(1);
    }
  }

  const result = { prs };
  const json = JSON.stringify(result, null, 2);

  if (outPath) {
    fs.writeFileSync(outPath, json, 'utf-8');
    console.error(`Wrote ${ prs.length } PR(s) to ${ path.resolve(outPath) }`);
  } else {
    console.log(json);
  }
}

main();
