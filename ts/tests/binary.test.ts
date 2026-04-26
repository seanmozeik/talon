import { describe, expect, it } from 'vitest';

import { resolveBinary } from '../src/binary';

describe('resolveBinary', () => {
  it.each([
    ['darwin', 'arm64', '@seanmozeik/talon-darwin-arm64/bin/talon'],
    ['darwin', 'x64', '@seanmozeik/talon-darwin-x64/bin/talon'],
    ['linux', 'arm64', '@seanmozeik/talon-linux-arm64/bin/talon'],
    ['linux', 'x64', '@seanmozeik/talon-linux-x64/bin/talon'],
    ['win32', 'x64', '@seanmozeik/talon-win32-x64/bin/talon'],
  ] as const)(
    'resolves %s-%s to its optional package',
    (platform, arch, packagePath) => {
      const resolved = resolveBinary({
        platform,
        arch,
        requireResolve: (id) => `/node_modules/${id}`,
      });

      expect(resolved).toBe(`/node_modules/${packagePath}`);
    },
  );

  it('rejects unsupported targets', () => {
    expect(() =>
      resolveBinary({
        platform: 'freebsd',
        arch: 'x64',
        requireResolve: (id) => id,
      }),
    ).toThrow('unsupported talon platform: freebsd-x64');
  });

  it('wraps missing optional dependency errors with package context', () => {
    expect(() =>
      resolveBinary({
        platform: 'linux',
        arch: 'x64',
        requireResolve: () => {
          throw new Error('module not found');
        },
      }),
    ).toThrow(
      'could not resolve optional dependency @seanmozeik/talon-linux-x64: module not found',
    );
  });
});
