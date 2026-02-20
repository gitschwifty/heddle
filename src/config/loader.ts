import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parse } from "smol-toml";
import { debug } from "../debug.ts";
import { getHeddleHome } from "./paths.ts";

export type ApprovalMode = "suggest" | "auto-edit" | "full-auto" | "plan" | "yolo";

const VALID_APPROVAL_MODES: ReadonlySet<string> = new Set(["suggest", "auto-edit", "full-auto", "plan", "yolo"]);

export interface HeddleConfig {
	// ── Identity ──
	apiKey?: string;

	// ── Provider/API ──
	model: string;
	weakModel?: string;
	editorModel?: string;
	maxTokens?: number;
	temperature?: number;
	baseUrl?: string;

	// ── Session ──
	systemPrompt?: string;
	approvalMode?: ApprovalMode;
	instructions?: string[];
	tools?: string[];
	doomLoopThreshold?: number;
	budgetLimit?: number;
}

/** Type aliases for consumer clarity. */
export type ProviderFields = Pick<
	HeddleConfig,
	"model" | "weakModel" | "editorModel" | "maxTokens" | "temperature" | "baseUrl"
>;
export type SessionFields = Pick<
	HeddleConfig,
	"systemPrompt" | "approvalMode" | "instructions" | "tools" | "doomLoopThreshold" | "budgetLimit"
>;

const DEFAULTS: HeddleConfig = {
	model: "openrouter/free",
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

	// String fields
	if (typeof raw.model === "string") config.model = raw.model;
	if (typeof raw.api_key === "string") config.apiKey = raw.api_key;
	if (typeof raw.system_prompt === "string") config.systemPrompt = raw.system_prompt;
	if (typeof raw.weak_model === "string") config.weakModel = raw.weak_model;
	if (typeof raw.editor_model === "string") config.editorModel = raw.editor_model;
	if (typeof raw.base_url === "string") config.baseUrl = raw.base_url;

	// Number fields
	if (typeof raw.max_tokens === "number") config.maxTokens = raw.max_tokens;
	if (typeof raw.temperature === "number") config.temperature = raw.temperature;
	if (typeof raw.doom_loop_threshold === "number") config.doomLoopThreshold = raw.doom_loop_threshold;
	if (typeof raw.budget_limit === "number") config.budgetLimit = raw.budget_limit;

	// Approval mode — validate against allowed values
	if (typeof raw.approval_mode === "string" && VALID_APPROVAL_MODES.has(raw.approval_mode)) {
		config.approvalMode = raw.approval_mode as ApprovalMode;
	}

	// Instructions — must be an array of strings
	if (Array.isArray(raw.instructions)) {
		const filtered = raw.instructions.filter((item): item is string => typeof item === "string");
		if (filtered.length > 0) config.instructions = filtered;
	}

	// Tools — must be an array of strings
	if (Array.isArray(raw.tools)) {
		const filtered = raw.tools.filter((item): item is string => typeof item === "string");
		if (filtered.length > 0) config.tools = filtered;
	}

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
	if (process.env.HEDDLE_BASE_URL) merged.baseUrl = process.env.HEDDLE_BASE_URL;

	// Numeric env vars with isNaN guard
	const maxTokensEnv = process.env.HEDDLE_MAX_TOKENS;
	if (maxTokensEnv && !Number.isNaN(Number(maxTokensEnv))) {
		merged.maxTokens = Number(maxTokensEnv);
	}

	const temperatureEnv = process.env.HEDDLE_TEMPERATURE;
	if (temperatureEnv && !Number.isNaN(Number(temperatureEnv))) {
		merged.temperature = Number(temperatureEnv);
	}

	if (process.env.HEDDLE_WEAK_MODEL) merged.weakModel = process.env.HEDDLE_WEAK_MODEL;

	const approvalModeEnv = process.env.HEDDLE_APPROVAL_MODE;
	if (approvalModeEnv && VALID_APPROVAL_MODES.has(approvalModeEnv)) {
		merged.approvalMode = approvalModeEnv as ApprovalMode;
	}

	const toolsEnv = process.env.HEDDLE_TOOLS;
	if (toolsEnv) {
		const parsed = toolsEnv
			.split(",")
			.map((t) => t.trim())
			.filter(Boolean);
		if (parsed.length > 0) merged.tools = parsed;
	}

	debug("config", "loaded:", merged);
	return merged;
}
