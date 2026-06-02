/**
 * Standalone unit tests for ShannonClient NDJSON parsing.
 *
 * Run: npx ts-node editors/vscode/src/test/suite/shannonClient.test.ts
 * Or:  node out/test/suite/shannonClient.test.js  (after compile)
 */

// ── Types and helpers (inline to avoid vscode module dependency) ─────────────

interface ShannonMessage {
  type: string;
  [key: string]: unknown;
}

function isTextDelta(msg: ShannonMessage): boolean { return msg.type === 'text_delta'; }
function isToolUse(msg: ShannonMessage): boolean { return msg.type === 'tool_use'; }
function isToolResult(msg: ShannonMessage): boolean { return msg.type === 'tool_result'; }
function isError(msg: ShannonMessage): boolean { return msg.type === 'error'; }
function isDone(msg: ShannonMessage): boolean { return msg.type === 'done'; }

function parseNDJSON(data: string): { messages: ShannonMessage[]; invalid: string[] } {
  const messages: ShannonMessage[] = [];
  const invalid: string[] = [];
  for (const line of data.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try { messages.push(JSON.parse(trimmed)); }
    catch { invalid.push(trimmed); }
  }
  return { messages, invalid };
}

// ── Test harness ────────────────────────────────────────────────────────────

let passed = 0;
let failed = 0;

function assert(condition: boolean, msg: string): void {
  if (!condition) throw new Error(`Assertion failed: ${msg}`);
}

function assertEqual(actual: unknown, expected: unknown, label: string): void {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function assertDeepEqual(actual: unknown, expected: unknown, label: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function test(name: string, fn: () => void): void {
  try {
    fn();
    passed++;
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.log(`  ✗ ${name}`);
    console.log(`    ${(e as Error).message}`);
  }
}

// ── NDJSON Parsing ──────────────────────────────────────────────────────────

console.log('NDJSON Parsing');

test('parses single text_delta', () => {
  const { messages, invalid } = parseNDJSON('{"type":"text_delta","content":"hello"}\n');
  assertEqual(messages.length, 1, 'count');
  assertEqual(messages[0].type, 'text_delta', 'type');
  assertEqual(messages[0].content, 'hello', 'content');
  assertEqual(invalid.length, 0, 'invalid');
});

test('parses multiple messages', () => {
  const data = '{"type":"text_delta","content":"hello"}\n{"type":"done","exit_code":0}\n';
  const { messages } = parseNDJSON(data);
  assertEqual(messages.length, 2, 'count');
  assertEqual(messages[0].content, 'hello', 'msg0');
  assertEqual(messages[1].exit_code, 0, 'exit_code');
});

test('skips non-JSON lines', () => {
  const data = 'not json\n{"type":"text_delta","content":"ok"}\nalso bad\n';
  const { messages, invalid } = parseNDJSON(data);
  assertEqual(messages.length, 1, 'valid count');
  assertEqual(invalid.length, 2, 'invalid count');
});

test('handles empty input', () => {
  const { messages, invalid } = parseNDJSON('');
  assertEqual(messages.length, 0, 'messages');
  assertEqual(invalid.length, 0, 'invalid');
});

test('handles blank lines', () => {
  const data = '\n\n{"type":"text_delta","content":"x"}\n\n';
  const { messages } = parseNDJSON(data);
  assertEqual(messages.length, 1, 'count');
});

test('parses tool_use with input', () => {
  const { messages } = parseNDJSON('{"type":"tool_use","name":"Bash","input":{"command":"ls"}}\n');
  assertEqual(messages[0].type, 'tool_use', 'type');
  assertEqual(messages[0].name, 'Bash', 'name');
  assertDeepEqual(messages[0].input, { command: 'ls' }, 'input');
});

test('parses error message', () => {
  const { messages } = parseNDJSON('{"type":"error","message":"API key required"}\n');
  assertEqual(messages[0].type, 'error', 'type');
  assertEqual(messages[0].message, 'API key required', 'message');
});

// ── Type guards ─────────────────────────────────────────────────────────────

console.log('\nType Guards');

test('isTextDelta', () => {
  assert(isTextDelta({ type: 'text_delta', content: 'x' }), 'positive');
  assert(!isTextDelta({ type: 'error', message: 'x' }), 'negative');
});

test('isToolUse', () => {
  assert(isToolUse({ type: 'tool_use', name: 'Bash', input: {} }), 'positive');
  assert(!isToolUse({ type: 'text_delta' }), 'negative');
});

test('isToolResult', () => {
  assert(isToolResult({ type: 'tool_result', name: 'Bash', output: 'ok', is_error: false }), 'positive');
  assert(!isToolResult({ type: 'text_delta' }), 'negative');
});

test('isError', () => {
  assert(isError({ type: 'error', message: 'fail' }), 'positive');
  assert(!isError({ type: 'text_delta' }), 'negative');
});

test('isDone', () => {
  assert(isDone({ type: 'done', exit_code: 0 }), 'positive');
  assert(!isDone({ type: 'text_delta' }), 'negative');
});

// ── Buffer simulation (chunked stream) ──────────────────────────────────────

console.log('\nBuffer Simulation');

test('handles chunked data', () => {
  const chunks = [
    '{"type":"text_',
    'delta","content":"hel',
    'lo"}\n{"type":"done","exit_',
    'code":0}\n',
  ];

  let buffer = '';
  const messages: ShannonMessage[] = [];

  for (const chunk of chunks) {
    buffer += chunk;
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';
    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      try { messages.push(JSON.parse(trimmed)); } catch { /* skip */ }
    }
  }

  assertEqual(messages.length, 2, 'count');
  assertEqual(messages[0].content, 'hello', 'content');
  assertEqual(messages[1].exit_code, 0, 'exit_code');
});

// ── Summary ─────────────────────────────────────────────────────────────────

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
  process.exit(1);
}
