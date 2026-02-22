#!/usr/bin/env bun
/**
 * Build script that injects compile-time constants (e.g. PROTOCOL_VERSION)
 * into heddle binaries via Bun's --define flag.
 *
 * Usage:
 *   bun run scripts/build.ts cli              # CLI binary (mac-arm64)
 *   bun run scripts/build.ts headless         # Headless binary (mac-arm64)
 *   bun run scripts/build.ts all              # Both
 *   bun run scripts/build.ts cli --target=bun-linux-x64
 *
 * TODO: Combine CLI and headless into a single binary with subcommand routing.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { $ } from "bun";

const ROOT = join(import.meta.dir, "..");
const protocolVersion = readFileSync(join(ROOT, "PROTOCOL_VERSION"), "utf-8").trim();

const ENTRYPOINTS: Record<string, { entry: string; outfile: string }> = {
	cli: { entry: "src/cli/index.ts", outfile: "dist/heddle" },
	headless: { entry: "src/headless/index.ts", outfile: "dist/heddle-headless" },
};

const args = process.argv.slice(2);
const targetFlag = args.find((a) => a.startsWith("--target="));
const target = targetFlag?.split("=")[1] ?? "bun-mac-arm64";
const modes = args.filter((a) => !a.startsWith("--"));

if (modes.length === 0 || (modes.length === 1 && modes[0] === "all")) {
	modes.length = 0;
	modes.push(...Object.keys(ENTRYPOINTS));
}

for (const mode of modes) {
	const config = ENTRYPOINTS[mode];
	if (!config) {
		console.error(`Unknown build target: ${mode}. Available: ${Object.keys(ENTRYPOINTS).join(", ")}`);
		process.exit(1);
	}

	const outfile = target !== "bun-mac-arm64" ? `${config.outfile}-${target.replace("bun-", "")}` : config.outfile;

	console.log(`Building ${mode} (${target})...`);
	await $`bun build --compile --no-env-file --target=${target} --define __PROTOCOL_VERSION__='"${protocolVersion}"' ${join(ROOT, config.entry)} --outfile ${join(ROOT, outfile)}`;
	console.log(`  → ${outfile}`);
}
