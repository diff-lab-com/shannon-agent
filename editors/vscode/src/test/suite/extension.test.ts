/**
 * Standalone unit tests for extension logic:
 * - Config building (buildEnv)
 * - Command autocomplete matching
 * - Message history management
 * - Binary path resolution helpers
 *
 * Wrapped in IIFE to avoid variable conflicts with shannonClient.test.ts.
 */

(function () {
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

// ── Config (buildEnv) ───────────────────────────────────────────────────────

function buildEnv(config: { apiKey: string; provider: string; model: string }): Record<string, string> {
  const env: Record<string, string> = {};
  if (config.apiKey) { env.SHANNON_API_KEY = config.apiKey; }
  if (config.provider) { env.SHANNON_PROVIDER = config.provider; }
  if (config.model) { env.SHANNON_MODEL = config.model; }
  return env;
}

console.log('Config (buildEnv)');

test('sets API key env var', () => {
  const env = buildEnv({ apiKey: 'sk-test', provider: 'anthropic', model: '' });
  assertEqual(env.SHANNON_API_KEY, 'sk-test', 'apiKey');
  assertEqual(env.SHANNON_PROVIDER, 'anthropic', 'provider');
  assert(!('SHANNON_MODEL' in env), 'model should be absent when empty');
});

test('omits empty values', () => {
  const env = buildEnv({ apiKey: '', provider: '', model: '' });
  assertEqual(Object.keys(env).length, 0, 'empty config should produce no env vars');
});

test('sets all env vars', () => {
  const env = buildEnv({ apiKey: 'sk-123', provider: 'openai', model: 'gpt-4' });
  assertEqual(env.SHANNON_API_KEY, 'sk-123', 'apiKey');
  assertEqual(env.SHANNON_PROVIDER, 'openai', 'provider');
  assertEqual(env.SHANNON_MODEL, 'gpt-4', 'model');
});

// ── Command autocomplete matching ───────────────────────────────────────────

const BUILTIN_COMMANDS = [
  '/help — Show available commands',
  '/config — View or change configuration',
  '/profile — Switch permission profile',
  '/model — Change active model',
  '/clear — Clear conversation history',
  '/compact — Compact conversation context',
  '/commit — Create a git commit',
  '/review — Review code changes',
  '/batch — Run parallel worktree tasks',
  '/team — Manage agent teams',
  '/routine — Manage routines',
  '/mcp — Manage MCP servers',
  '/doctor — Check Shannon installation',
];

console.log('\nCommand Autocomplete');

test('matches /h to /help', () => {
  const matches = BUILTIN_COMMANDS.filter(c => c.toLowerCase().startsWith('/h'));
  assertEqual(matches.length, 1, 'count');
  assert(matches[0].startsWith('/help'), 'should match help');
});

test('matches /c to /clear, /compact, /config, /commit', () => {
  const matches = BUILTIN_COMMANDS.filter(c => c.toLowerCase().startsWith('/c'));
  assert(matches.length >= 3, 'should match multiple /c commands');
});

test('matches /co to /config and /compact', () => {
  const matches = BUILTIN_COMMANDS.filter(c => c.toLowerCase().startsWith('/co'));
  assert(matches.length >= 2, 'should match config and compact');
});

test('no match for /xyz', () => {
  const matches = BUILTIN_COMMANDS.filter(c => c.toLowerCase().startsWith('/xyz'));
  assertEqual(matches.length, 0, 'no matches for /xyz');
});

test('exact match returns single', () => {
  const matches = BUILTIN_COMMANDS.filter(c => c.toLowerCase().startsWith('/help'));
  assertEqual(matches.length, 1, 'exact match');
});

test('non-slash prefix returns empty', () => {
  const matches = BUILTIN_COMMANDS.filter(c => c.toLowerCase().startsWith('hello'));
  assertEqual(matches.length, 0, 'non-slash returns empty');
});

// ── Message history management ──────────────────────────────────────────────

interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

console.log('\nMessage History');

test('messages accumulate correctly', () => {
  const messages: ChatMessage[] = [];
  messages.push({ role: 'user', content: 'hello' });
  messages.push({ role: 'assistant', content: 'hi there' });
  messages.push({ role: 'system', content: 'done.' });
  assertEqual(messages.length, 3, 'count');
  assertEqual(messages[0].role, 'user', 'msg0 role');
  assertEqual(messages[1].content, 'hi there', 'msg1 content');
});

test('assistant streaming accumulates', () => {
  const messages: ChatMessage[] = [];
  messages.push({ role: 'assistant', content: 'Hello' });
  const last = messages[messages.length - 1];
  if (last?.role === 'assistant') {
    last.content += ' world';
  }
  assertEqual(messages.length, 1, 'still one message');
  assertEqual(messages[0].content, 'Hello world', 'accumulated content');
});

test('clear history empties array', () => {
  const messages: ChatMessage[] = [
    { role: 'user', content: 'test' },
    { role: 'assistant', content: 'response' },
  ];
  messages.length = 0;
  assertEqual(messages.length, 0, 'cleared');
});

test('mixed roles track correctly', () => {
  const messages: ChatMessage[] = [];
  messages.push({ role: 'user', content: 'q1' });
  messages.push({ role: 'assistant', content: 'a1' });
  messages.push({ role: 'user', content: 'q2' });
  messages.push({ role: 'assistant', content: 'a2' });
  messages.push({ role: 'system', content: 'done' });
  const roles = messages.map(m => m.role);
  assertEqual(roles.join(','), 'user,assistant,user,assistant,system', 'role order');
});

// ── Binary path validation ──────────────────────────────────────────────────

console.log('\nBinary Path Validation');

test('configured path takes priority', () => {
  const configuredPath: string | undefined = '/usr/local/bin/shannon';
  const useConfigured = configuredPath && configuredPath !== 'shannon';
  assert(!!useConfigured, 'configured path should be used');
});

test('default "shannon" triggers PATH search', () => {
  const configuredPath = 'shannon';
  const useConfigured = configuredPath && configuredPath !== 'shannon';
  assert(!useConfigured, 'default should trigger PATH search');
});

test('empty path triggers PATH search', () => {
  const configuredPath = '';
  const useConfigured = configuredPath && configuredPath !== 'shannon';
  assert(!useConfigured, 'empty should trigger PATH search');
});

// ── Summary ─────────────────────────────────────────────────────────────────

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
  process.exit(1);
}

})();
