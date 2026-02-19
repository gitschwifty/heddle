import { debug } from "../debug.ts";

export type ReasoningEffort = "xhigh" | "high" | "medium" | "low" | "minimal" | "none";
export type ReasoningSummaryVerbosity = "auto" | "concise" | "detailed";

export interface OpenRouterOverrides {
	// Model & routing
	model?: string;
	models?: string[];
	route?: "fallback" | "sort";

	// Sampling
	temperature?: number;
	max_tokens?: number;
	top_p?: number;
	seed?: number;
	stop?: string | string[];
	frequency_penalty?: number;
	presence_penalty?: number;

	// Reasoning
	reasoning?: {
		effort?: ReasoningEffort;
		max_tokens?: number;
		excluded?: boolean;
		summary?: ReasoningSummaryVerbosity;
	};

	// Response format
	response_format?: { type: string; json_schema?: Record<string, unknown> };

	// Tools
	tools?: Array<{
		type: "function";
		function: { name: string; description?: string; parameters?: Record<string, unknown> };
	}>;
	tool_choice?: "auto" | "none" | "required" | { type: "function"; function: { name: string } };

	// Plugins
	plugins?: Array<{
		id: "auto-router" | "moderation" | "web" | "file-parser" | "response-healing";
		enabled?: boolean;
		[key: string]: unknown;
	}>;

	// Provider preferences
	provider?: {
		allow_fallbacks?: boolean;
		order?: string[];
		only?: string[];
		ignore?: string[];
		require_parameters?: boolean;
		data_collection?: "allow" | "deny";
		sort?: string | { by?: string; partition?: string };
		[key: string]: unknown;
	};

	// Observability
	session_id?: string;

	// Debug
	debug?: { echo_upstream_body?: boolean };
}

const VALID_REASONING_EFFORTS = new Set<string>(["xhigh", "high", "medium", "low", "minimal", "none"]);
const VALID_REASONING_SUMMARIES = new Set<string>(["auto", "concise", "detailed"]);

const KNOWN_KEYS = new Set<string>([
	"model",
	"models",
	"route",
	"temperature",
	"max_tokens",
	"top_p",
	"seed",
	"stop",
	"frequency_penalty",
	"presence_penalty",
	"reasoning",
	"response_format",
	"tools",
	"tool_choice",
	"plugins",
	"provider",
	"session_id",
	"debug",
]);

/**
 * Validate and filter override fields. Does NOT throw — filters bad values and warns via debug.
 * Returns a clean OpenRouterOverrides object.
 */
export function validateOverrides(raw: Record<string, unknown>): OpenRouterOverrides {
	const result: OpenRouterOverrides = {};

	// Warn on unknown keys
	for (const key of Object.keys(raw)) {
		if (!KNOWN_KEYS.has(key)) {
			debug("provider", `unknown override key: "${key}"`);
		}
	}

	// String fields
	if (typeof raw.model === "string") result.model = raw.model;
	if (typeof raw.session_id === "string") {
		if (raw.session_id.length <= 128) {
			result.session_id = raw.session_id;
		} else {
			debug("provider", "session_id exceeds 128 chars, ignoring");
		}
	}

	// Route
	if (raw.route === "fallback" || raw.route === "sort") result.route = raw.route;

	// Models array
	if (Array.isArray(raw.models) && raw.models.every((m) => typeof m === "string")) {
		result.models = raw.models as string[];
	}

	// Numeric fields with range validation
	if (typeof raw.temperature === "number") {
		if (raw.temperature >= 0 && raw.temperature <= 2) {
			result.temperature = raw.temperature;
		} else {
			debug("provider", `temperature ${raw.temperature} out of range [0, 2], ignoring`);
		}
	}

	if (typeof raw.max_tokens === "number") {
		if (raw.max_tokens > 0 && Number.isInteger(raw.max_tokens)) {
			result.max_tokens = raw.max_tokens;
		} else {
			debug("provider", `max_tokens ${raw.max_tokens} must be positive integer, ignoring`);
		}
	}

	if (typeof raw.top_p === "number") result.top_p = raw.top_p;
	if (typeof raw.seed === "number") result.seed = raw.seed;
	if (typeof raw.frequency_penalty === "number") result.frequency_penalty = raw.frequency_penalty;
	if (typeof raw.presence_penalty === "number") result.presence_penalty = raw.presence_penalty;

	// Stop
	if (typeof raw.stop === "string" || (Array.isArray(raw.stop) && raw.stop.every((s) => typeof s === "string"))) {
		result.stop = raw.stop as string | string[];
	}

	// Reasoning (nested object)
	if (raw.reasoning && typeof raw.reasoning === "object" && !Array.isArray(raw.reasoning)) {
		const r = raw.reasoning as Record<string, unknown>;
		const reasoning: OpenRouterOverrides["reasoning"] = {};
		let hasField = false;

		if (typeof r.effort === "string" && VALID_REASONING_EFFORTS.has(r.effort)) {
			reasoning.effort = r.effort as ReasoningEffort;
			hasField = true;
		}
		if (typeof r.max_tokens === "number" && r.max_tokens > 0 && Number.isInteger(r.max_tokens)) {
			reasoning.max_tokens = r.max_tokens;
			hasField = true;
		}
		if (typeof r.excluded === "boolean") {
			reasoning.excluded = r.excluded;
			hasField = true;
		}
		if (typeof r.summary === "string" && VALID_REASONING_SUMMARIES.has(r.summary)) {
			reasoning.summary = r.summary as ReasoningSummaryVerbosity;
			hasField = true;
		}

		if (hasField) result.reasoning = reasoning;
	}

	// Pass-through complex objects (validated structurally but not deeply)
	if (raw.response_format && typeof raw.response_format === "object") {
		result.response_format = raw.response_format as OpenRouterOverrides["response_format"];
	}
	if (Array.isArray(raw.tools)) {
		result.tools = raw.tools as OpenRouterOverrides["tools"];
	}
	if (raw.tool_choice !== undefined) {
		result.tool_choice = raw.tool_choice as OpenRouterOverrides["tool_choice"];
	}
	if (Array.isArray(raw.plugins)) {
		result.plugins = raw.plugins as OpenRouterOverrides["plugins"];
	}
	if (raw.provider && typeof raw.provider === "object" && !Array.isArray(raw.provider)) {
		result.provider = raw.provider as OpenRouterOverrides["provider"];
	}
	if (raw.debug && typeof raw.debug === "object" && !Array.isArray(raw.debug)) {
		result.debug = raw.debug as OpenRouterOverrides["debug"];
	}

	return result;
}
