import type { ToolCategory } from "./checker.ts";

export interface PermissionRule {
	tool: string; // tool name, or "*"
	pattern?: string; // glob pattern (optional)
}

export interface PermissionConfig {
	allow: PermissionRule[];
	deny: PermissionRule[];
	ask: PermissionRule[];
}

// ── Tool name mapping ──────────────────────────────────────────────────
// Maps PascalCase display names to internal tool names
const TOOL_NAME_MAP: Record<string, string> = {
	read: "read_file",
	write: "write_file",
	edit: "edit_file",
	bash: "bash",
	glob: "glob",
	grep: "grep",
	webfetch: "web_fetch",
	askuser: "ask_user",
	savememory: "save_memory",
};

// Known internal tool names (already snake_case)
const KNOWN_TOOLS = new Set([
	"read_file",
	"write_file",
	"edit_file",
	"bash",
	"glob",
	"grep",
	"web_fetch",
	"ask_user",
	"save_memory",
]);

const CATEGORY_TOOLS: Record<ToolCategory, string[]> = {
	read: ["read_file", "glob", "grep", "ask_user"],
	write: ["write_file", "edit_file", "save_memory"],
	execute: ["bash"],
	network: ["web_fetch"],
};

const CATEGORY_NAMES = new Set(Object.keys(CATEGORY_TOOLS));

// Tools whose primary arg is a file path
const PATH_TOOLS = new Set(["read_file", "write_file", "edit_file", "glob", "grep"]);

// ── Glob cache ─────────────────────────────────────────────────────────
const globCache = new Map<string, InstanceType<typeof Bun.Glob>>();

function getGlob(pattern: string): InstanceType<typeof Bun.Glob> {
	let g = globCache.get(pattern);
	if (!g) {
		g = new Bun.Glob(pattern);
		globCache.set(pattern, g);
	}
	return g;
}

function globMatch(pattern: string, str: string): boolean {
	return getGlob(pattern).match(str);
}

// ── Helpers ────────────────────────────────────────────────────────────
function basename(filePath: string): string {
	const parts = filePath.split("/");
	return parts[parts.length - 1] ?? filePath;
}

/**
 * Match a command pattern against a command string.
 * "rm *" matches any command starting with "rm " (prefix match).
 * "rm" matches exactly "rm".
 * "git status" matches exactly "git status".
 */
function matchCommand(pattern: string, command: string): boolean {
	// Pattern ending with " *" → prefix match (everything starting with that prefix)
	if (pattern.endsWith(" *")) {
		const prefix = pattern.slice(0, -1); // keep the trailing space: "rm "
		return command.startsWith(prefix) || command === pattern.slice(0, -2);
	}
	// Exact match
	return command === pattern;
}

function extractHostname(url: string): string | null {
	try {
		return new URL(url).hostname;
	} catch {
		return null;
	}
}

function resolveToolName(name: string): string | null {
	const lower = name.toLowerCase();
	// Check if it's already a known internal name
	if (KNOWN_TOOLS.has(lower)) return lower;
	// Check display name map (case-insensitive, no separators)
	const normalized = lower.replace(/[_-]/g, "");
	if (TOOL_NAME_MAP[normalized]) return TOOL_NAME_MAP[normalized];
	return null;
}

// ── Parse ──────────────────────────────────────────────────────────────

/**
 * Parse a rule string like "Write(src/**)" into PermissionRule(s).
 * Returns null for invalid strings.
 * Returns an array for category names (expands to all tools in category).
 */
export function parseRule(raw: string): PermissionRule | PermissionRule[] | null {
	const trimmed = raw.trim();
	if (!trimmed) return null;

	// Wildcard
	if (trimmed === "*") return { tool: "*" };

	// Extract name and optional pattern
	let name: string;
	let pattern: string | undefined;

	const parenOpen = trimmed.indexOf("(");
	if (parenOpen !== -1) {
		if (!trimmed.endsWith(")")) return null; // unclosed paren
		name = trimmed.slice(0, parenOpen);
		pattern = trimmed.slice(parenOpen + 1, -1);
	} else {
		name = trimmed;
	}

	const lowerName = name.toLowerCase();

	// Lowercase names that match a category are treated as categories
	// (e.g., "write" → category, "Write" → tool name write_file)
	if (name === lowerName && CATEGORY_NAMES.has(lowerName)) {
		const tools = CATEGORY_TOOLS[lowerName as ToolCategory];
		return tools.map((tool) => (pattern ? { tool, pattern } : { tool }));
	}

	// Resolve as specific tool name
	const toolName = resolveToolName(name);
	if (toolName) {
		return pattern ? { tool: toolName, pattern } : { tool: toolName };
	}

	// Unknown name — return null
	return null;
}

// ── Match ──────────────────────────────────────────────────────────────

/** Does this rule match the given tool call? */
export function matchRule(rule: PermissionRule, toolName: string, args?: Record<string, unknown>): boolean {
	// Tool name match
	if (rule.tool !== "*" && rule.tool !== toolName) return false;

	// No pattern = bare name match (any invocation)
	if (!rule.pattern) return true;

	// Pattern requires args to match against
	if (!args) return false;

	// File path tools: match against path arg (full path + basename)
	if (PATH_TOOLS.has(toolName) && args.path) {
		const filePath = String(args.path);
		return globMatch(rule.pattern, filePath) || globMatch(rule.pattern, basename(filePath));
	}

	// Bash: match against command string using prefix matching
	// Glob * doesn't cross / which is wrong for commands, so use startsWith for "cmd *" patterns
	if (toolName === "bash" && args.command) {
		const cmd = String(args.command);
		return matchCommand(rule.pattern, cmd);
	}

	// Web fetch: match against hostname
	if (toolName === "web_fetch" && args.url) {
		const hostname = extractHostname(String(args.url));
		if (hostname) return globMatch(rule.pattern, hostname);
	}

	return false;
}

// ── Evaluate ───────────────────────────────────────────────────────────

/**
 * Evaluate rules against a tool call.
 * Priority: deny > ask > allow.
 * Returns null if no rules match.
 */
export function evaluateRules(
	config: PermissionConfig,
	toolName: string,
	args?: Record<string, unknown>,
): "allow" | "deny" | "ask" | null {
	const denyMatch = config.deny.some((r) => matchRule(r, toolName, args));
	if (denyMatch) return "deny";

	const askMatch = config.ask.some((r) => matchRule(r, toolName, args));
	if (askMatch) return "ask";

	const allowMatch = config.allow.some((r) => matchRule(r, toolName, args));
	if (allowMatch) return "allow";

	return null;
}

// ── Merge ──────────────────────────────────────────────────────────────

/**
 * Merge permission configs from multiple layers (global → local → overrides).
 * More specific layers (later args) can override less specific layers:
 * - If a later layer explicitly allows something a prior layer denied (same tool+pattern),
 *   the deny is removed so the allow takes effect.
 * - If a later layer explicitly denies something a prior layer allowed,
 *   the deny takes precedence (deny wins within merged result).
 */
export function mergeConfigs(...configs: PermissionConfig[]): PermissionConfig {
	if (configs.length === 0) return { allow: [], deny: [], ask: [] };
	if (configs.length === 1) {
		const c = configs[0] as PermissionConfig;
		return { allow: [...c.allow], deny: [...c.deny], ask: [...c.ask] };
	}

	// Track accumulated rules, allowing later layers to override
	const mergedDeny: PermissionRule[] = [];
	const mergedAllow: PermissionRule[] = [];
	const mergedAsk: PermissionRule[] = [];

	for (const config of configs) {
		// Later-layer allows can remove prior-layer denies (same tool+pattern override)
		for (const allowRule of config.allow) {
			// Remove matching denies from earlier layers
			const idx = mergedDeny.findIndex((d) => d.tool === allowRule.tool && d.pattern === allowRule.pattern);
			if (idx !== -1) {
				mergedDeny.splice(idx, 1);
			}
			mergedAllow.push(allowRule);
		}

		// Later-layer denies always stack
		for (const denyRule of config.deny) {
			mergedDeny.push(denyRule);
		}

		// Ask rules stack
		for (const askRule of config.ask) {
			mergedAsk.push(askRule);
		}
	}

	return { allow: mergedAllow, deny: mergedDeny, ask: mergedAsk };
}
