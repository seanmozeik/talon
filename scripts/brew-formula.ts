#!/usr/bin/env -S bun run

/**
 * Render Formula/talon.rb from the tarballs in dist/.
 *
 * Expects `just dist` to have produced:
 *   dist/talon-darwin-arm64.tar.gz
 *   dist/talon-darwin-x64.tar.gz
 *   dist/talon-linux-arm64.tar.gz
 *   dist/talon-linux-x64.tar.gz
 *
 * Each tarball contains a single executable at the root named
 * `talon-<label>`, which `bin.install` renames to `talon`.
 *
 * Usage:
 *   bun scripts/brew-formula.ts --version 0.4.2
 */

import { $ } from "bun";
import fs from "node:fs/promises";

const argv = process.argv.slice(2);
const args: Record<string, string> = {};
for (let i = 0; i < argv.length; i++) {
  const arg = argv[i];
  if (!arg?.startsWith("--")) continue;
  const key = arg.replace(/^--/, "");
  const next = argv[i + 1];
  if (next && !next.startsWith("-")) {
    args[key] = argv[++i] as string;
  } else {
    args[key] = "true";
  }
}

const version = args.version ?? process.env.VERSION;
if (!version) {
  console.error("error: --version <semver> required");
  process.exit(1);
}

const LABELS = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"] as const;
type Label = (typeof LABELS)[number];

async function sha256(path: string): Promise<string> {
  const out = (await $`shasum -a 256 ${path}`.text()).trim();
  const hash = out.split(/\s+/)[0];
  if (!hash || hash.length !== 64) {
    throw new Error(`bad shasum output for ${path}: ${out}`);
  }
  return hash;
}

const shas: Record<Label, string> = {} as Record<Label, string>;
for (const label of LABELS) {
  const path = `dist/talon-${label}.tar.gz`;
  if (!(await Bun.file(path).exists())) {
    console.error(`error: ${path} not found — run \`just dist\` first`);
    process.exit(1);
  }
  shas[label] = await sha256(path);
  console.log(`  ${label}  ${shas[label]}`);
}

const formula = `class Talon < Formula
  desc "Hybrid retrieval for Obsidian vaults: BM25 + semantic + reranker, with grounded answers and MCP"
  homepage "https://github.com/seanmozeik/talon"
  version "${version}"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-darwin-arm64.tar.gz"
      sha256 "${shas["darwin-arm64"]}"
    else
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-darwin-x64.tar.gz"
      sha256 "${shas["darwin-x64"]}"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-linux-arm64.tar.gz"
      sha256 "${shas["linux-arm64"]}"
    else
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-linux-x64.tar.gz"
      sha256 "${shas["linux-x64"]}"
    end
  end

  def install
    if OS.mac?
      binary_name = Hardware::CPU.arm? ? "talon-darwin-arm64" : "talon-darwin-x64"
    else
      binary_name = Hardware::CPU.arm? ? "talon-linux-arm64" : "talon-linux-x64"
    end
    bin.install binary_name => "talon"
  end

  test do
    assert_match "talon", shell_output("#{bin}/talon --version")
  end
end
`;

await fs.mkdir("Formula", { recursive: true });
await fs.writeFile("Formula/talon.rb", formula);
console.log(`\n==> wrote Formula/talon.rb (v${version})`);
