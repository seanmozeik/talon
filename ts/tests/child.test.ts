import { describe, expect, it, vi } from 'vitest';

describe('mcpChildSpec', () => {
  it('returns the resolved binary with --mcp args and process env', async () => {
    vi.doMock('../src/binary', () => ({
      resolveBinary: () => '/tmp/talon/bin/talon',
    }));

    const { mcpChildSpec } = await import('../src/child');
    const spec = mcpChildSpec();

    expect(spec).toEqual({
      command: '/tmp/talon/bin/talon',
      args: ['--mcp'],
      env: process.env,
    });
  });
});
