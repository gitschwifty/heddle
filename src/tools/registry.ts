import type { ToolDefinition } from "../types.ts";
import type { HeddleTool } from "./types.ts";

export class ToolRegistry {
	private tools = new Map<string, HeddleTool>();

	register(tool: HeddleTool): void {
		if (this.tools.has(tool.name)) {
			throw new Error(`Tool "${tool.name}" already registered`);
		}
		this.tools.set(tool.name, tool);
	}

	get(name: string): HeddleTool | undefined {
		return this.tools.get(name);
	}

	all(): HeddleTool[] {
		return [...this.tools.values()];
	}

	/** Generate OpenAI-format tool definitions for the API. */
	definitions(): ToolDefinition[] {
		return this.all().map((tool) => ({
			type: "function" as const,
			function: {
				name: tool.name,
				description: tool.description,
				parameters: tool.parameters,
			},
		}));
	}

	/** Execute a tool by name, parsing JSON string arguments. */
	async execute(name: string, argsJson: string): Promise<string> {
		const tool = this.tools.get(name);
		if (!tool) {
			throw new Error(`Unknown tool: ${name}`);
		}

		let parsed: unknown;
		try {
			parsed = JSON.parse(argsJson);
		} catch {
			return `Error: Invalid JSON arguments: ${argsJson}`;
		}

		try {
			return await tool.execute(parsed);
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			return `Error: ${message}`;
		}
	}
}
