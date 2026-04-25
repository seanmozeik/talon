import { resolveBinary } from './binary';
import type { McpChildSpec } from './types';

export const mcpChildSpec = (): McpChildSpec => ({
  command: resolveBinary(),
  args: ['--mcp'],
  env: {},
});
