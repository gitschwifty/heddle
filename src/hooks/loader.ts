import type { HookEvent, ResolvedHookDefinition, ResolvedHooksConfig } from "./types.ts";

const VALID_EVENTS = new Set<string>([
	"session_start",
	"session_end",
	"pre_prompt",
	"pre_tool",
	"post_tool",
	"post_turn",
	"error",
]);

/** Parse a single hook entry from raw TOML/JSON, applying defaults. */
function toHookDefinition(raw: unknown): ResolvedHookDefinition | null {
	if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
	const r = raw as Record<string, unknown>;
	if (typeof r.command !== "string") return null;

	const def: ResolvedHookDefinition = {
		command: r.command,
		timeout: typeof r.timeout === "number" ? r.timeout : 10000,
		mode: r.mode === "interactive" || r.mode === "headless" || r.mode === "both" ? r.mode : "both",
		async: typeof r.async === "boolean" ? r.async : false,
	};

	// Parse matchers if present
	if (r.matchers && typeof r.matchers === "object" && !Array.isArray(r.matchers)) {
		const m = r.matchers as Record<string, unknown>;
		const matchers: Record<string, unknown> = {};
		let hasMatchers = false;

		if (typeof m.tool === "string" || (Array.isArray(m.tool) && m.tool.every((t) => typeof t === "string"))) {
			matchers.tool = m.tool;
			hasMatchers = true;
		}
		if (typeof m.match_path === "string") {
			matchers.match_path = m.match_path;
			hasMatchers = true;
		}
		if (typeof m.match_args === "string") {
			matchers.match_args = m.match_args;
			hasMatchers = true;
		}
		if (typeof m.match_input === "string") {
			matchers.match_input = m.match_input;
			hasMatchers = true;
		}

		if (hasMatchers) {
			def.matchers = matchers as ResolvedHookDefinition["matchers"];
		}
	}

	return def;
}

/** Extract hooks from a raw config object's [hooks] section. */
function extractHooks(raw: Record<string, unknown>): ResolvedHooksConfig {
	const hooksRaw = raw.hooks;
	if (!hooksRaw || typeof hooksRaw !== "object" || Array.isArray(hooksRaw)) return {};

	const h = hooksRaw as Record<string, unknown>;
	const config: ResolvedHooksConfig = {};

	for (const [eventName, entries] of Object.entries(h)) {
		if (!VALID_EVENTS.has(eventName)) continue;
		if (!Array.isArray(entries)) continue;

		const parsed: ResolvedHookDefinition[] = [];
		for (const entry of entries) {
			const def = toHookDefinition(entry);
			if (def) parsed.push(def);
		}

		if (parsed.length > 0) {
			config[eventName as HookEvent] = parsed;
		}
	}

	return config;
}

/**
 * Load hooks from global and local raw TOML objects, merging additively.
 * Global hooks come first, local hooks are appended per event.
 */
export function loadHooks(globalRaw: Record<string, unknown>, localRaw: Record<string, unknown>): ResolvedHooksConfig {
	const globalHooks = extractHooks(globalRaw);
	const localHooks = extractHooks(localRaw);

	const merged: ResolvedHooksConfig = { ...globalHooks };

	for (const eventName of VALID_EVENTS) {
		const key = eventName as HookEvent;
		const localArr = localHooks[key];
		if (!localArr) continue;

		const existing = merged[key];
		if (existing) {
			merged[key] = [...existing, ...localArr];
		} else {
			merged[key] = localArr;
		}
	}

	return merged;
}

/**
 * Merge file-based hooks with IPC hooks.
 * IPC hooks override for the same event (replace, not append).
 */
export function mergeHooksWithIpc(fileHooks: ResolvedHooksConfig, ipcHooks: ResolvedHooksConfig): ResolvedHooksConfig {
	const merged: ResolvedHooksConfig = { ...fileHooks };

	for (const eventName of VALID_EVENTS) {
		const key = eventName as HookEvent;
		if (ipcHooks[key]) {
			merged[key] = ipcHooks[key];
		}
	}

	return merged;
}
