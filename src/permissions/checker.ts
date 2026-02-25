import type { ApprovalMode } from "../config/loader.ts";
import type { ToolDefinition } from "../types.ts";

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

const ENV_FILE_RE = /^\.env(\..+)?$/;
const RM_COMMAND_RE = /(?:^|\s)rm\s/;

function basename(filePath: string): string {
	const parts = filePath.split("/");
	return parts[parts.length - 1] ?? filePath;
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

	constructor(mode: ApprovalMode) {
		this.mode = mode;
	}

	check(toolName: string, args?: Record<string, unknown>): PermissionDecision {
		// Hardcoded protections — always checked first
		const hardcoded = this.checkHardcoded(toolName, args);
		if (hardcoded) return hardcoded;

		// Session-scoped allowlist
		if (this.alwaysAllowed.has(toolName)) {
			return { decision: "allow" };
		}

		const category = TOOL_CATEGORIES[toolName] ?? "execute";
		const matrix = MODE_MATRIX[this.mode];
		const decision = matrix[category];

		if (decision === "allow") {
			return { decision: "allow" };
		}

		const reason = `${toolName} (${category}) requires approval in ${this.mode} mode`;
		return { decision, reason };
	}

	allowAlways(toolName: string): void {
		this.alwaysAllowed.add(toolName);
	}

	private checkHardcoded(toolName: string, args?: Record<string, unknown>): PermissionDecision | null {
		// .env file write protection
		if ((toolName === "write_file" || toolName === "edit_file") && args?.path) {
			const name = basename(String(args.path));
			if (ENV_FILE_RE.test(name)) {
				return { decision: "deny", reason: `Writing to .env files is not allowed: ${name}` };
			}
		}

		// rm command protection
		if (toolName === "bash" && args?.command) {
			const cmd = String(args.command);
			if (RM_COMMAND_RE.test(cmd)) {
				return { decision: "deny", reason: "rm commands are not allowed — use trash instead" };
			}
		}

		return null;
	}
}
