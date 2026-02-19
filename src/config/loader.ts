import { join } from "node:path";
import { existsSync, readFileSync } from "node:fs";
import { parse } from "smol-toml";
import { getHeddleHome } from "./paths.ts";

export interface HeddleConfig {
	model: string;
	apiKey?: string;
	systemPrompt?: string;
}

const DEFAULTS: HeddleConfig = {
	model: "moonshotai/kimi-k2.5",
};

/** Parse a TOML file into a plain object. Returns {} on missing/malformed files. */
function loadToml(path: string): Record<string, unknown> {
	if (!existsSync(path)) return {};
	try {
		const content = readFileSync(path, "utf-8");
		if (!content.trim()) return {};
		return parse(content) as Record<string, unknown>;
	} catch {
		return {};
	}
}

/** Map raw TOML keys to HeddleConfig fields. */
function toConfig(raw: Record<string, unknown>): Partial<HeddleConfig> {
	const config: Partial<HeddleConfig> = {};
	if (typeof raw.model === "string") config.model = raw.model;
	if (typeof raw.api_key === "string") config.apiKey = raw.api_key;
	if (typeof raw.system_prompt === "string") config.systemPrompt = raw.system_prompt;
	return config;
}

/**
 * Load config with layered merging: defaults → global → local → env vars.
 * @param localDir Path to the local .heddle directory (defaults to cwd/.heddle)
 */
export function loadConfig(localDir?: string): HeddleConfig {
	const globalPath = join(getHeddleHome(), "config.toml");
	const localPath = localDir ? join(localDir, "config.toml") : undefined;

	const globalRaw = loadToml(globalPath);
	const localRaw = localPath ? loadToml(localPath) : {};

	const merged: HeddleConfig = {
		...DEFAULTS,
		...toConfig(globalRaw),
		...toConfig(localRaw),
	};

	// Env vars override everything
	if (process.env.HEDDLE_MODEL) merged.model = process.env.HEDDLE_MODEL;
	if (process.env.OPENROUTER_API_KEY) merged.apiKey = process.env.OPENROUTER_API_KEY;

	return merged;
}
