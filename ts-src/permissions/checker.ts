import type { ApprovalMode } from "../config/loader.ts";
import type { ToolDefinition } from "../types.ts";
import { evaluateRules, mergeConfigs, type PermissionConfig, parseRule } from "./rules.ts";

export type ToolCategory = "read" | "write" | "execute" | "network";

export type PermissionDecision = {
	decision: "allow" | "deny" | "ask";
	reason?: string;
};

const TOOL_CATEGORIES: Record<string, ToolCategory> = {
	read_file: "read",
	glob: "read",
	grep: "read",
	ask_user: "read",
	write_file: "write",
	edit_file: "write",
	save_memory: "write",
	bash: "execute",
	web_fetch: "network",
};

type ModeMatrix = Record<ToolCategory, "allow" | "deny" | "ask">;

const MODE_MATRIX: Record<ApprovalMode, ModeMatrix> = {
	plan: { read: "allow", network: "allow", write: "deny", execute: "deny" },
	suggest: { read: "allow", network: "allow", write: "ask", execute: "ask" },
	"auto-edit": { read: "allow", network: "allow", write: "allow", execute: "ask" },
	"full-auto": { read: "allow", network: "allow", write: "allow", execute: "allow" },
	yolo: { read: "allow", network: "allow", write: "allow", execute: "allow" },
};

// Tools whose args contain a file path (used for directory scoping)
const PATH_ARG_TOOLS = new Set(["read_file", "write_file", "edit_file", "glob", "grep"]);

export interface PermissionCheckerOptions {
	layers?: Array<{ allow: string[]; deny: string[]; ask: string[] }>;
	projectDir?: string;
}

/** Filter tool definitions to only read/network categories. */
export function readOnlyToolFilter(tools: ToolDefinition[]): ToolDefinition[] {
	return tools.filter((t) => {
		const category = TOOL_CATEGORIES[t.function.name] ?? "execute";
		return category === "read" || category === "network";
	});
}

export class PermissionChecker {
	private mode: ApprovalMode;
	private alwaysAllowed = new Set<string>();
	private mergedRules: PermissionConfig | null;
	private projectDir?: string;

	constructor(mode: ApprovalMode, options?: PermissionCheckerOptions) {
		this.mode = mode;
		this.projectDir = options?.projectDir;

		// Parse and merge rule layers
		if (options?.layers && options.layers.length > 0) {
			const parsed = options.layers.map((layer) => this.parseLayer(layer));
			this.mergedRules = mergeConfigs(...parsed);
		} else {
			this.mergedRules = null;
		}
	}

	check(toolName: string, args?: Record<string, unknown>): PermissionDecision {
		// 1. Yolo mode — allow everything, skip all checks
		if (this.mode === "yolo") {
			return { decision: "allow" };
		}

		// 2-4. Evaluate rules (deny > ask > allow)
		if (this.mergedRules) {
			const ruleDecision = evaluateRules(this.mergedRules, toolName, args);
			if (ruleDecision === "deny") {
				return { decision: "deny", reason: this.ruleReason(toolName, "deny") };
			}
			if (ruleDecision === "ask") {
				return { decision: "ask", reason: this.ruleReason(toolName, "ask") };
			}
			if (ruleDecision === "allow") {
				return { decision: "allow" };
			}
		}

		// 5. Directory scoping — downgrade if outside project dir (file-path tools only)
		const dirDowngrade = this.shouldDowngrade(toolName, args);

		// 6. Session allowlist
		if (this.alwaysAllowed.has(toolName)) {
			// Even session allowlist respects dir scoping
			if (dirDowngrade) {
				return { decision: "ask", reason: `${toolName} targets path outside project directory` };
			}
			return { decision: "allow" };
		}

		// 7. Mode matrix — fallback
		const category = TOOL_CATEGORIES[toolName] ?? "execute";
		const matrix = MODE_MATRIX[this.mode];
		let decision = matrix[category];

		// Apply dir scoping downgrade to mode matrix result
		if (dirDowngrade && decision === "allow") {
			decision = "ask";
		}

		if (decision === "allow") {
			return { decision: "allow" };
		}

		const reason = `${toolName} (${category}) requires approval in ${this.mode} mode`;
		return { decision, reason };
	}

	allowAlways(toolName: string): void {
		this.alwaysAllowed.add(toolName);
	}

	private parseLayer(layer: { allow: string[]; deny: string[]; ask: string[] }): PermissionConfig {
		const parseRules = (rules: string[]) => {
			const result: PermissionConfig["allow"] = [];
			for (const raw of rules) {
				const parsed = parseRule(raw);
				if (parsed === null) continue;
				if (Array.isArray(parsed)) {
					result.push(...parsed);
				} else {
					result.push(parsed);
				}
			}
			return result;
		};

		return {
			allow: parseRules(layer.allow),
			deny: parseRules(layer.deny),
			ask: parseRules(layer.ask),
		};
	}

	private shouldDowngrade(toolName: string, args?: Record<string, unknown>): boolean {
		if (!this.projectDir) return false;
		if (!PATH_ARG_TOOLS.has(toolName)) return false;
		if (!args?.path) return false;

		return !this.isInsideProject(String(args.path));
	}

	private isInsideProject(filePath: string): boolean {
		if (!this.projectDir) return true;

		// Resolve relative paths against cwd
		const { resolve } = require("node:path");
		const resolved = resolve(filePath);
		const projectResolved = resolve(this.projectDir);

		return resolved.startsWith(`${projectResolved}/`) || resolved === projectResolved;
	}

	private ruleReason(toolName: string, decision: "deny" | "ask"): string {
		if (decision === "deny") {
			return `${toolName} blocked by deny rule`;
		}
		return `${toolName} requires confirmation (ask rule)`;
	}
}
