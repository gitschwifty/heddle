import { readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";
import type { DiscoveryLevel, DiscoveryResult } from "./discovery.ts";

export interface Skill {
	name: string;
	description: string;
	content: string;
	frontmatter: Record<string, string>;
	source: string;
	level: DiscoveryLevel;
}

/**
 * Parse YAML-style frontmatter from a string.
 * Expects `---` delimiters. Returns empty frontmatter if none found.
 * Uses regex-based parsing — no YAML library needed.
 */
export function parseFrontmatter(content: string): {
	frontmatter: Record<string, string>;
	body: string;
} {
	const match = content.match(/^---\s*\n([\s\S]*?\n)?---\s*\n?([\s\S]*)$/);
	if (!match) {
		return { frontmatter: {}, body: content };
	}

	const rawFrontmatter = match[1] ?? "";
	const body = (match[2] ?? "").trim();
	const frontmatter: Record<string, string> = {};

	for (const line of rawFrontmatter.split("\n")) {
		const kvMatch = line.match(/^\s*([^:]+?)\s*:\s*(.+?)\s*$/);
		if (kvMatch?.[1] && kvMatch[2]) {
			frontmatter[kvMatch[1]] = kvMatch[2];
		}
	}

	return { frontmatter, body };
}

/**
 * Parse a single skill file into a Skill object.
 *
 * @param filePath - Absolute path to the .md file
 * @param namespace - Subdirectory path relative to skills root (e.g., "foo/bar")
 * @param level - The DiscoveryLevel this skill belongs to
 */
export function parseSkillFile(filePath: string, namespace: string, level: DiscoveryLevel): Skill {
	const content = readFileSync(filePath, "utf-8");
	const { frontmatter, body } = parseFrontmatter(content);

	// Derive name from path: namespace + filename without .md, separated by colons
	const basename = filePath.replace(/\.md$/, "").split("/").pop() ?? "";
	const nameParts = namespace ? [...namespace.split("/"), basename] : [basename];
	const name = nameParts.join(":");

	const description = frontmatter.description ?? `Custom skill: ${name}`;

	return {
		name,
		description,
		content: body,
		frontmatter,
		source: level.path,
		level,
	};
}

/**
 * Recursively scan a directory for .md skill files.
 * Returns Skill objects with names derived from relative paths.
 */
function scanSkillsDir(dir: string, level: DiscoveryLevel, baseDir?: string): Skill[] {
	const base = baseDir ?? dir;
	const skills: Skill[] = [];

	try {
		const entries = readdirSync(dir);
		for (const entry of entries) {
			const fullPath = join(dir, entry);
			try {
				const stat = statSync(fullPath);
				if (stat.isDirectory()) {
					skills.push(...scanSkillsDir(fullPath, level, base));
					continue;
				}
				if (extname(entry) !== ".md") continue;

				const relDir = relative(base, dir);
				const skill = parseSkillFile(fullPath, relDir, level);
				skills.push(skill);
			} catch {
				// Skip files we can't read
			}
		}
	} catch {
		// Directory unreadable — skip
	}

	return skills;
}

/**
 * Load all skills from a DiscoveryResult.
 * Skills from earlier levels (deepest .heddle/) take priority over later levels.
 * When names collide, the first occurrence wins (deepest/highest priority).
 */
export function loadSkillsFromDiscovery(discovery: DiscoveryResult): Skill[] {
	const skillMap = new Map<string, Skill>();

	for (const level of discovery.levels) {
		let skillsDir: string;
		if (level.source === "agents") {
			// .agents/skills/ — the level path IS the skills dir
			skillsDir = level.path;
		} else {
			skillsDir = join(level.path, "skills");
		}

		const skills = scanSkillsDir(skillsDir, level);
		for (const skill of skills) {
			if (!skillMap.has(skill.name)) {
				skillMap.set(skill.name, skill);
			}
		}
	}

	return [...skillMap.values()];
}
