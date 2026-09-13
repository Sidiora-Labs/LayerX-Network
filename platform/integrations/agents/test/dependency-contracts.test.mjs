import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import { AIMessage, AIMessageChunk } from '@langchain/core/messages';
import { DynamicTool } from '@langchain/core/tools';
import { RunTree } from 'langsmith/run_trees';
const require = createRequire(import.meta.url);
const { z } = require('zod/v3');

test('metadata presence preserves real schema validation', () => {
  const bare = z.object({ input: z.string().optional() }).transform(({ input }) => input);
  const described = bare.describe('A URL');
  for (const value of [{ input: 'https://example.com' }, {}, { input: 123 }, null]) {
    const left = bare.safeParse(value);
    const right = described.safeParse(value);
    assert.equal(left.success, right.success);
    if (left.success && right.success) assert.deepEqual(left.data, right.data);
    else assert.deepEqual(left.error.issues, right.error.issues);
  }
  assert.equal(bare.description, undefined);
  assert.equal(described.description, 'A URL');
});

test('message metadata is omitted when absent and retained when supplied', () => {
  const usage = { input_tokens: 1, output_tokens: 2, total_tokens: 3 };
  for (const Message of [AIMessage, AIMessageChunk]) {
    assert.equal(Object.hasOwn(new Message({ content: 'hello' }), 'usage_metadata'), false);
    assert.deepEqual(new Message({ content: 'hello', usage_metadata: usage }).usage_metadata, usage);
  }
  const run = new RunTree({ name: 'metadata', run_type: 'chain', tracingEnabled: false });
  assert.equal(Object.hasOwn(run, 'events'), false);
});

test('actual tool rejects invalid inputs and executes its URL parser', async () => {
  const tool = new DynamicTool({ name: 'url-host', description: 'Parse a URL', func: async (value) => new URL(value).hostname });
  assert.equal(await tool.invoke('https://example.com/path'), 'example.com');
  await assert.rejects(tool.invoke({ input: 123 }));
});
