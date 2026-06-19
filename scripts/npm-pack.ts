#!/usr/bin/env -S bun run

/**
 * Rust → npm platform-package packager.
 *
 * Takes already-built binaries from cargo's target directory and packages
 * them into npm/<label>/. Generates all npm files from Cargo.toml metadata.
 *
 * Does NOT build. Does NOT publish. Just packages.
 *
 * Usage:
 *   bun scripts/npm-pack.ts [options]
 *
 * Options:
 *   --npm-org <org>         NPM org prefix (e.g. "seanmozeik"). Omit for unscoped packages.
 *   --package <name>        Cargo package name (auto-detected if one bin crate)
 *   --binary <name>         Binary name in platform packages (default: strip -cli suffix)
 *   --targets <json>        Override target triples JSON array
 *   --skip-smoke            Skip smoke tests
 *   --require-smoke         Fail if can't smoke-test cross-platform builds
 *   --max-bytes <n>         Reject binaries larger than this (default: 30MB)
 *   --access <public|restricted>
 *                           publishConfig.access for scoped packages (default: public)
 */

import { $ } from "bun";
import fs from "node:fs/promises";
import binaryTemplate from "./binary.js.txt";

// ── Defaults ───────────────────────────────────────────────────────────────────

interface TargetDef {
  label: string;
  triple: string;
  npmOs: string;
  npmCpu: string;
}

const DEFAULT_TARGETS: TargetDef[] = [
  { label: "darwin-arm64", npmCpu: "arm64", npmOs: "darwin", triple: "aarch64-apple-darwin" },
  { label: "darwin-x64", npmCpu: "x64", npmOs: "darwin", triple: "x86_64-apple-darwin" },
  { label: "linux-arm64", npmCpu: "arm64", npmOs: "linux", triple: "aarch64-unknown-linux-gnu" },
  { label: "linux-x64", npmCpu: "x64", npmOs: "linux", triple: "x86_64-unknown-linux-gnu" },
  { label: "win32-x64", npmCpu: "x64", npmOs: "win32", triple: "x86_64-pc-windows-gnu" },
];

// ── Helpers ────────────────────────────────────────────────────────────────────

function hostOs(): string {
  return process.platform === "darwin"
    ? "darwin"
    : (process.platform === "linux"
      ? "linux"
      : "unknown");
}

function hostArch(): string {
  const a = process.arch as string;
  return a === "arm64" ? "arm64" : (a === "x64" ? "x64" : "unknown");
}

// ── CLI arg parsing ────────────────────────────────────────────────────────────

const args: Record<string, string | undefined> = {};
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  const arg = argv[i];
  if (!arg || !arg.startsWith("--")) {continue;}
  const key = arg.replace(/^--/, "");
  const next = argv[i + 1];
  if (next && !next.startsWith("-")) {
    args[key] = argv[++i];
  } else {
    args[key] = "true";
  }
}

// ── Config ─────────────────────────────────────────────────────────────────────

const npmOrg = (args["npm-org"] ?? process.env.NPM_ORG) || null;
const access = (args["access"] ?? "public") as "public" | "restricted";

function scopedName(base: string): string {
  return npmOrg ? `@${npmOrg}/${base}` : base;
}

function npmRepositoryUrl(): string {
  const source =
    cargoMeta.repository ??
    (npmOrg ? `https://github.com/${npmOrg}/${packageName}` : `https://github.com/${packageName}`);
  const gitUrl = source.startsWith("git+") ? source : `git+${source}`;
  return gitUrl.endsWith(".git") ? gitUrl : `${gitUrl}.git`;
}

// ── Resolve Cargo metadata ─────────────────────────────────────────────────────

interface CargoPackage {
  name: string;
  version: string;
  description?: string;
  license?: string;
  repository?: string;
  homepage?: string;
}

async function loadCargoMetadata(): Promise<CargoPackage> {
  const meta = JSON.parse(await $`cargo metadata --format-version 1 --no-deps`.text());

  const filterName = args.package;
  let pkg: (typeof meta.packages)[0];

  if (filterName) {
    pkg = meta.packages.find((p: { name: string }) => p.name === filterName);
    if (!pkg) {
      console.error(`error: package "${filterName}" not found in workspace`);
      process.exit(1);
    }
  } else {
    const binCrates = meta.packages.filter((p: { targets: { kind: string[] }[] }) =>
      p.targets.some((t: { kind: string[] }) => t.kind.includes("bin")),
    );
    if (binCrates.length === 0) {
      console.error("error: no binary crates found in workspace");
      process.exit(1);
    }
    if (binCrates.length > 1 && !args.package) {
      const names = binCrates.map((p: { name: string }) => p.name).join(", ");
      console.error(`error: multiple binary crates found (${names}), specify --package`);
      process.exit(1);
    }
    pkg = binCrates[0];
  }

  return {
    description: pkg.description,
    homepage: pkg.homepage,
    license: pkg.license,
    name: pkg.name,
    repository: pkg.repository,
    version: pkg.version,
  };
}

function deriveBinaryName(cargoPkgName: string): string {
  if (args.binary) {
    return args.binary;
  }
  return cargoPkgName.replace(/[-_.]?(?:cli|bin)$/, "");
}

const cargoMeta = await loadCargoMetadata();
const packageName = cargoMeta.name;
const { version } = cargoMeta;
const binaryName = deriveBinaryName(packageName);
const repository = { type: "git", url: npmRepositoryUrl() };
const skipSmoke = args["skip-smoke"] === "true" || process.env.TALON_SKIP_SMOKE === "1";
const requireSmoke =
  args["require-smoke"] === "true" || process.env.TALON_REQUIRE_TARGET_SMOKE === "1";
const maxBytes = Number.parseInt(args["max-bytes"] ?? "31457280", 10);

// Merge targets with binary name
const targetsArg = args.targets ? JSON.parse(args.targets) : null;
const targets = (targetsArg as TargetDef[]) ?? DEFAULT_TARGETS.map((t) => ({ ...t }));

// ── Main ───────────────────────────────────────────────────────────────────────

async function main() {
  const targetDir = JSON.parse(
    await $`cargo metadata --format-version 1 --no-deps`.text(),
  ).target_directory;

  console.log(`version=${version} package=${packageName} binary=${binaryName}`);
  if (cargoMeta.description) {
    console.log(`description=${cargoMeta.description}`);
  }
  if (cargoMeta.license) {
    console.log(`license=${cargoMeta.license}`);
  }
  if (cargoMeta.repository) {
    console.log(`repository=${cargoMeta.repository}`);
  }
  console.log(`targetDir=${targetDir}`);

  // ── Package each platform ──────────────────────────────────────────────

  for (const t of targets) {
    const outBinaryName = t.npmOs === "win32" ? `${binaryName}.exe` : binaryName;
    const buildBinary = `${targetDir}/${t.triple}/release/${outBinaryName}`;

    // Also try without .exe suffix (cargo may produce bare name on some targets)
    const altBinary = `${targetDir}/${t.triple}/release/${binaryName}`;

    let found: string | null = null;
    try {
      await fs.access(buildBinary);
      found = buildBinary;
    } catch {
      /* Not found */
    }
    if (!found) {
      try {
        await fs.access(altBinary);
        found = altBinary;
      } catch {
        /* Not found */
      }
    }

    if (!found) {
      console.error(`error: binary not found for ${t.label}:`);
      console.error(`  tried: ${buildBinary}`);
      console.error(`  tried: ${altBinary}`);
      process.exit(1);
    }

    const pkgDir = `npm/${t.label}`;
    await fs.rm(pkgDir, { force: true, recursive: true });
    await fs.mkdir(`${pkgDir}/bin`, { recursive: true });

    // Copy binary
    await Bun.write(`${pkgDir}/bin/${outBinaryName}`, await Bun.file(found).arrayBuffer());
    await $`chmod 0755 ${pkgDir}/bin/${outBinaryName}`;

    const { size } = await Bun.file(`${pkgDir}/bin/${outBinaryName}`).stat();
    if (size > maxBytes) {
      console.error(`${t.label}: ${size} bytes exceeds limit ${maxBytes}`);
      process.exit(1);
    }
    console.log(`  packaged ${t.label} (${size} bytes)`);

    // Write platform package.json
    const platformPkgName = scopedName(`${binaryName}-${t.label}`);
    const pkgJson: Record<string, unknown> = {
      bin: { [binaryName]: `bin/${outBinaryName}` },
      cpu: [t.npmCpu],
      description: "Prebuilt binary.",
      files: [`bin/${outBinaryName}`],
      license: cargoMeta.license || "MIT OR Apache-2.0",
      name: platformPkgName,
      os: [t.npmOs],
      private: false,
      publishConfig: { access },
      repository,
      type: "module",
      version,
    };
    await Bun.write(`${pkgDir}/package.json`, `${JSON.stringify(pkgJson, null, 2)}\n`);

    // Smoke test
    if (skipSmoke) {
      console.log(`  smoke skipped for ${t.label}`);
      continue;
    }

    const binaryPath = `${pkgDir}/bin/${outBinaryName}`;
    const isHost = hostOs() === t.npmOs && hostArch() === t.npmCpu;

    if (isHost) {
      await $`${binaryPath} --version`;
      console.log(`  smoke passed (${t.label})`);
      continue;
    }

    // Cross-platform smoke via qemu or wine
    if (t.npmOs === "linux" && t.npmCpu === "arm64") {
      const hasQemu = await $`command -v qemu-aarch64`.nothrow().text();
      if (hasQemu) {
        await $`qemu-aarch64 ${binaryPath} --version`;
        console.log(`  smoke passed via qemu (${t.label})`);
        continue;
      }
    }

    if (t.npmOs === "win32" && t.npmCpu === "x64") {
      const hasWine = await $`command -v wine`.nothrow().text();
      if (hasWine) {
        await $`wine ${found} --version`;
        console.log(`  smoke passed via wine (${t.label})`);
        continue;
      }
    }

    if (requireSmoke) {
      console.error(`no target runtime available to smoke test ${t.label}`);
      process.exit(1);
    }
    console.log(`  smoke skipped for ${t.label}; run on target OS or install qemu/wine`);
  }

  // ── Copy README into the package dir ───────────────────────────────────
  // npm only bundles a README that lives in the published package directory,
  // and publish runs from npm/. Without this copy the npm page is blank.

  const bundledFiles = ["binary.js"];
  try {
    await fs.copyFile("README.md", "npm/README.md");
    bundledFiles.push("README.md");
    console.log("==> copied README.md into npm/");
  } catch {
    console.warn("warning: README.md not found at repo root; npm page will be blank");
  }

  // ── Write main package.json ────────────────────────────────────────────

  const mainPkgJson: Record<string, unknown> = {
    bin: { [binaryName]: "binary.js" },
    description: cargoMeta.description ?? "Prebuilt binary.",
    files: bundledFiles,
    homepage: cargoMeta.homepage ?? cargoMeta.repository,
    name: scopedName(binaryName),
    optionalDependencies: Object.fromEntries(
      targets.map((t) => [scopedName(`${binaryName}-${t.label}`), version]),
    ),
    private: false,
    publishConfig: { access },
    repository,
    type: "module",
    version,
    workspaces: targets.map((t) => t.label),
  };
  await Bun.write(`npm/package.json`, `${JSON.stringify(mainPkgJson, null, 2)}\n`);
  console.log(`\n==> written npm/package.json`);

  // ── Write binary.js resolver ───────────────────────────────────────────

  const platformMap = Object.fromEntries(
    targets.map((t) => [`${t.npmOs}:${t.npmCpu}`, scopedName(`${binaryName}-${t.label}`)]),
  );

  const binaryJs = binaryTemplate
    .replace("{{PLATFORMS}}", JSON.stringify(platformMap, null, 2))
    .replace("{{BINARY}}", binaryName);

  await Bun.write(`npm/binary.js`, binaryJs);
  await $`chmod 0755 npm/binary.js`;
  console.log(`==> written npm/binary.js`);

  console.log("\n==> done — all files in npm/");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
