/* eslint-disable no-console */
const { execSync } = require('child_process');
const dependencyTree = require('dependency-tree');
const fs = require('fs');
const path = require('path');

const ROOT_DIR = path.resolve(__dirname, '../../../');

const TARGET_FILE = path.resolve(ROOT_DIR, './playwright/affected-tests.txt');

const NON_EXISTENT_DEPS = [];

const DIRECTORIES_WITH_TESTS = [
  path.resolve(ROOT_DIR, './ui'),
];
const VISITED = {};

function getAllPwFilesInDirectory(directory) {
  const files = fs.readdirSync(directory, { recursive: true });
  return files
    .filter((file) => file.endsWith('.pw.tsx'))
    .map((file) => path.join(directory, file));
}

function getFileDeps(filename, changedNpmModules) {
  return dependencyTree.toList({
    filename,
    directory: ROOT_DIR,
    filter: (path) => {
      if (path.indexOf('node_modules') === -1) {
        return true;
      }

      if (changedNpmModules.some((module) => path.startsWith(module))) {
        return true;
      }

      return false;
    },
    tsConfig: path.resolve(ROOT_DIR, './tsconfig.json'),
    nonExistent: NON_EXISTENT_DEPS,
    visited: VISITED,
  });
}

async function getChangedFiles() {
  const command = process.env.CI ?
    `git diff --name-only origin/${ process.env.GITHUB_BASE_REF } ${ process.env.GITHUB_SHA } -- ${ ROOT_DIR }` :
    `git diff --name-only main $(git branch --show-current) -- ${ ROOT_DIR }`;

  console.log('Executing command: ', command);
  const files = execSync(command)
    .toString()
    .trim()
    .split('\n')
    .filter(Boolean);

  return files.map((file) => path.join(ROOT_DIR, file));
}

function checkChangesInChakraTheme(changedFiles) {
  const themeDir = path.resolve(ROOT_DIR, './theme');
  return changedFiles.some((file) => file.startsWith(themeDir));
}

function checkChangesInSvgSprite(changedFiles) {
  const iconDir = path.resolve(ROOT_DIR, './icons');
  const areIconsChanged = changedFiles.some((file) => file.startsWith(iconDir));

  if (!areIconsChanged) {
    return false;
  }

  const svgNamesFile = path.resolve(ROOT_DIR, './public/icons/name.d.ts');
  const areSvgNamesChanged = changedFiles.some((file) => file === svgNamesFile);

  if (!areSvgNamesChanged) {
    // If only the icons have changed and not the names in the SVG file, we will need to run all tests.
    // This is because we cannot correctly identify the test files that depend on these changes.
    return true;
  }

  // If the icon names have changed, then there should be changes in the components that use them.
  // Otherwise, typescript would complain about that.
  return false;
}

function createTargetFile(content) {
  fs.writeFileSync(TARGET_FILE, content);
}

function getPackageJsonUpdatedProps(packageJsonFile) {
  const command = process.env.CI ?
    `git diff --unified=0 origin/${ process.env.GITHUB_BASE_REF } ${ process.env.GITHUB_SHA } -- ${ packageJsonFile }` :
    `git diff --unified=0 main $(git branch --show-current) -- ${ packageJsonFile }`;

  console.log('Executing command: ', command);
  const changedLines = execSync(command)
    .toString()
    .trim()
    .split('\n')
    .filter(Boolean)
    .filter((line) => line.startsWith('+ ') || line.startsWith('- '));

  const changedProps = [ ...new Set(
    changedLines
      .map((line) => line.replaceAll(' ', '').replaceAll('+', '').replaceAll('-', ''))
      .map((line) => line.split(':')[0].replaceAll('"', '')),
  ) ];

  return changedProps;
}

function getUpdatedNpmModules(changedFiles) {
  const packageJsonFile = path.resolve(ROOT_DIR, './package.json');

  if (!changedFiles.includes(packageJsonFile)) {
    return [];
  }

  try {
    const packageJsonContent = JSON.parse(fs.readFileSync(packageJsonFile, 'utf-8'));
    const usedNpmModules = [
      ...Object.keys(packageJsonContent.dependencies || {}),
      ...Object.keys(packageJsonContent.devDependencies || {}),
    ];
    const updatedProps = getPackageJsonUpdatedProps(packageJsonFile);

    return updatedProps.filter((prop) => usedNpmModules.includes(prop));
  } catch (error) {}
}

async function run() {
  // NOTES:
  // - The absence of TARGET_FILE implies that all tests should be run.
  // - The empty TARGET_FILE implies that no tests should be run.

  const start = Date.now();

  fs.unlink(TARGET_FILE, () => {});

  const changedFiles = await getChangedFiles();

  if (!changedFiles.length) {
    createTargetFile('');
    console.log('No changed files found. Exiting...');
    return;
  }

  console.log('Changed files in the branch: ', changedFiles);

  if (checkChangesInChakraTheme(changedFiles)) {
    console.log('Changes in Chakra theme detected. It is advisable to run all test suites. Exiting...');
    return;
  }

  if (checkChangesInSvgSprite(changedFiles)) {
    console.log('There are some changes in the SVG sprite that cannot be linked to a specific component. It is advisable to run all test suites. Exiting...');
    return;
  }

  let changedNpmModules = getUpdatedNpmModules(changedFiles);

  if (!changedNpmModules) {
    console.log('Some error occurred while detecting changed NPM modules. It is advisable to run all test suites. Exiting...');
    return;
  }

  console.log('Changed NPM modules in the branch: ', changedNpmModules);

  changedNpmModules = [
    ...changedNpmModules,
    ...changedNpmModules.map((module) => `@types/${ module }`), // there are some deps that are resolved to .d.ts files
  ].map((module) => path.resolve(ROOT_DIR, `./node_modules/${ module }`));

  const allTestFiles = DIRECTORIES_WITH_TESTS.reduce((acc, dir) => {
    return acc.concat(getAllPwFilesInDirectory(dir));
  }, []);

  const isDepChanged = (dep) => changedFiles.includes(dep) || changedNpmModules.some((module) => dep.startsWith(module));

  const testFilesToRun = allTestFiles
    .map((file) => ({ file, deps: getFileDeps(file, changedNpmModules) }))
    .filter(({ deps }) => deps.some(isDepChanged));
  const testFileNamesToRun = testFilesToRun.map(({ file }) => path.relative(ROOT_DIR, file));

  if (!testFileNamesToRun.length) {
    createTargetFile('');
    console.log('No tests to run. Exiting...');
    return;
  }

  createTargetFile(testFileNamesToRun.join('\n'));

  const end = Date.now();

  const testFilesToRunWithFilteredDeps = testFilesToRun.map(({ file, deps }) => ({
    file,
    deps: deps.filter(isDepChanged),
  }));

  console.log('Total time: ', ((end - start) / 1_000).toLocaleString());
  console.log('Total test to run: ', testFileNamesToRun.length);
  console.log('Tests to run with changed deps: ', testFilesToRunWithFilteredDeps);
  console.log('Non existent deps: ', NON_EXISTENT_DEPS);
}

run();
