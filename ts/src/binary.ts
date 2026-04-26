import { createRequire } from 'node:module';

type SupportedPlatform = 'darwin' | 'linux' | 'win32';
type SupportedArch = 'arm64' | 'x64';

interface ResolveBinaryOptions {
  readonly platform?: NodeJS.Platform;
  readonly arch?: NodeJS.Architecture;
  readonly requireResolve?: (id: string) => string;
}

const require = createRequire(import.meta.url);

const packageByTarget: ReadonlyMap<string, string> = new Map([
  ['darwin:arm64', '@seanmozeik/talon-darwin-arm64'],
  ['darwin:x64', '@seanmozeik/talon-darwin-x64'],
  ['linux:arm64', '@seanmozeik/talon-linux-arm64'],
  ['linux:x64', '@seanmozeik/talon-linux-x64'],
  ['win32:x64', '@seanmozeik/talon-win32-x64'],
]);

const isSupportedPlatform = (
  platform: NodeJS.Platform,
): platform is SupportedPlatform =>
  platform === 'darwin' || platform === 'linux' || platform === 'win32';

const isSupportedArch = (arch: NodeJS.Architecture): arch is SupportedArch =>
  arch === 'arm64' || arch === 'x64';

export const resolveBinary = (options: ResolveBinaryOptions = {}): string => {
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const requireResolve = options.requireResolve ?? require.resolve;

  if (!isSupportedPlatform(platform) || !isSupportedArch(arch)) {
    throw new Error(`unsupported talon platform: ${platform}-${arch}`);
  }

  const packageName = packageByTarget.get(`${platform}:${arch}`);
  if (packageName === undefined) {
    throw new Error(`unsupported talon platform: ${platform}-${arch}`);
  }

  try {
    return requireResolve(`${packageName}/bin/talon`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      `could not resolve optional dependency ${packageName}: ${message}`,
    );
  }
};
