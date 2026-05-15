import { readFileSync } from "node:fs";
import { basename, join } from "node:path";
import matter from "gray-matter";
import type { DiscoveryResult } from "../config/discovery.ts";
import type { AgentDefinition } from "./types.ts";

/**
 * Parse a single agent definition file (markdown with YAML frontmatter).
 * Returns null if the file doesn't exist, is empty, or has malformed frontmatter.
 */
export function parseAgentFile(filePath: string): AgentDefinition | null {
	let raw: string;
	try {
		raw = readFileSync(filePath, "utf-8");
	} catch {
		return null;
	}

	if (raw.trim().length === 0) {
		return null;
	}

	let parsed: matter.GrayMatterFile<string>;
	try {
		parsed = matter(raw);
	} catch {
		return null;
	}

	const data = parsed.data as Record<string, unknown>;
	const body = parsed.content.trim();

	const name = typeof data.name === "string" ? data.name : basename(filePath, ".md");

	const description = typeof data.description === "string" ? data.description : "";

	const model = typeof data.model === "string" ? data.model : undefined;

	const tools = Array.isArray(data.tools) ? (data.tools.filter((t) => typeof t === "string") as string[]) : undefined;

	return {
		name,
		description,
		model,
		tools,
		systemPrompt: body,
		source: filePath,
	};
}

/**
 * Load agent definitions from a DiscoveryResult.
 * Levels are deepest-first, so project-level agents override global ones.
 * Returns a Map keyed by agent name.
 */
export function loadAgentDefinitions(discovery: DiscoveryResult): Map<string, AgentDefinition> {
	const agents = new Map<string, AgentDefinition>();

	for (const level of discovery.levels) {
		for (const filename of level.agents) {
			const filePath = join(level.path, "agents", filename);
			const agent = parseAgentFile(filePath);
			if (agent === null) {
				continue;
			}
			// Deepest-first: first definition wins (don't overwrite)
			if (!agents.has(agent.name)) {
				agents.set(agent.name, agent);
			}
		}
	}

	return agents;
}
