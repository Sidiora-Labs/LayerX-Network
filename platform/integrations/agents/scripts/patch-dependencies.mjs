import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { readFile, writeFile } from 'node:fs/promises';
const require = createRequire(import.meta.url);
const patches = JSON.parse(await readFile(new URL('./dependency-patches.json', import.meta.url), 'utf8'));
const digest = (value) => createHash('sha256').update(value).digest('hex');
function packageRoot(name, parent = require) {
  let path = dirname(parent.resolve(name));
  for (;;) {
    try {
      const metadata = parent(join(path, 'package.json'));
      if (metadata.name === name) return { path, version: metadata.version };
    } catch (error) {
      if (error.code !== 'MODULE_NOT_FOUND') throw error;
    }
    const next = dirname(path);
    if (next === path) throw new Error(`Missing package root: ${name}`);
    path = next;
  }
}
const core = packageRoot('@langchain/core');
const providers = {
  '@langchain/core': core,
  '@ai-sdk/provider-utils': packageRoot('@ai-sdk/provider-utils', createRequire(join(packageRoot('ai').path, 'package.json'))),
  langsmith: packageRoot('langsmith', createRequire(join(core.path, 'package.json'))),
};
for (const patch of patches) {
  const dependency = providers[patch.package];
  if (dependency.version !== patch.version) throw new Error(`Dependency version changed: ${patch.package}`);
  const path = join(dependency.path, patch.path);
  let text = await readFile(path, 'utf8');
  if (digest(text) === patch.after) continue;
  if (digest(text) !== patch.before) throw new Error(`Dependency bytes changed: ${patch.package}/${patch.path}`);
  for (const [before, after] of patch.replacements) text = text.replaceAll(before, after);
  if (digest(text) !== patch.after) throw new Error(`Dependency patch mismatch: ${patch.package}/${patch.path}`);
  await writeFile(path, text);
}
