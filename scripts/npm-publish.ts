#!/usr/bin/env -S bun run

/**
 * Publish the generated npm/ package tree.
 *
 * Usage:
 *   bun scripts/npm-publish.ts all [--dry-run]
 *   bun scripts/npm-publish.ts platforms [--dry-run]
 *   bun scripts/npm-publish.ts platform <label> [--dry-run]
 *   bun scripts/npm-publish.ts root [--dry-run]
 *
 * Set NPM_OTP to pass one OTP to every npm publish invocation. If NPM_OTP is
 * unset and stdin is interactive, the script prompts once.
 */

import { spawnSync } from "node:child_process";
import { createInterface } from "node:readline/promises";

type Mode = "all" | "platforms" | "platform" | "root";

const rawArgs = process.argv.slice(2);
const dryRun = rawArgs.includes("--dry-run");
const positional = rawArgs.filter((arg) => arg !== "--dry-run");
const mode = (positional[0] ?? "all") as Mode;
const platform = positional[1];

if (!["all", "platforms", "platform", "root"].includes(mode)) {
  console.error(`error: unknown publish mode "${mode}"`);
  usage();
  process.exit(2);
}

if (mode === "platform" && !platform) {
  console.error("error: platform mode requires a platform label");
  usage();
  process.exit(2);
}

const publishFlags = await buildPublishFlags();

switch (mode) {
  case "all": {
    publishPlatformWorkspaces();
    publishRoot();
    break;
  }
  case "platforms": {
    publishPlatformWorkspaces();
    break;
  }
  case "platform": {
    publishSinglePlatform(platform as string);
    break;
  }
  case "root": {
    publishRoot();
    break;
  }
}

async function buildPublishFlags(): Promise<string[]> {
  const flags = dryRun ? ["--dry-run"] : [];
  const otp = process.env.NPM_OTP ?? (await promptOtp());
  if (otp) {
    flags.push("--otp", otp);
  }
  return flags;
}

async function promptOtp(): Promise<string | null> {
  if (dryRun || !process.stdin.isTTY) {
    return null;
  }
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  try {
    const answer = await rl.question("npm OTP (blank to let npm prompt): ");
    return answer.trim() || null;
  } finally {
    rl.close();
  }
}

function publishPlatformWorkspaces(): void {
  console.log("==> Publishing npm platform workspaces...");
  runNpmPublish(["--workspaces"]);
}

function publishSinglePlatform(label: string): void {
  console.log(`==> Publishing npm platform ${label}...`);
  runNpmPublish([`./${label}`]);
}

function publishRoot(): void {
  console.log("==> Publishing npm root package...");
  runNpmPublish(["."]);
}

function runNpmPublish(args: string[]): void {
  const result = spawnSync("npm", ["publish", ...args, ...publishFlags], {
    cwd: "npm",
    stdio: "inherit",
  });
  if (result.error) {
    console.error(`error: failed to run npm publish: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function usage(): void {
  console.error(
    "usage: bun scripts/npm-publish.ts all|platforms|root|platform <label> [--dry-run]",
  );
}
