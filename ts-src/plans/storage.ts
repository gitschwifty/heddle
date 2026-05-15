import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { getProjectDir } from "../config/paths.ts";

/** Sanitize a plan name: strip path separators and dots to prevent traversal. */
function sanitizeName(name: string): string {
	return name.replace(/[/\\]/g, "-").replace(/\.\./g, "").replace(/^\.+/, "");
}

/** Plans directory: getProjectDir() + "/plans/". */
export function getPlansDir(projectPath?: string): string {
	return join(getProjectDir(projectPath), "plans");
}

/** Parse YAML-style frontmatter from a markdown string. */
function parseFrontmatter(raw: string): { meta: Record<string, string>; body: string } {
	const meta: Record<string, string> = {};

	if (!raw.startsWith("---\n")) {
		return { meta, body: raw };
	}

	const endIndex = raw.indexOf("\n---\n", 4);
	if (endIndex === -1) {
		return { meta, body: raw };
	}

	const frontmatterBlock = raw.slice(4, endIndex);
	for (const line of frontmatterBlock.split("\n")) {
		const colonIdx = line.indexOf(":");
		if (colonIdx === -1) continue;
		const key = line.slice(0, colonIdx).trim();
		const value = line.slice(colonIdx + 1).trim();
		if (key) {
			meta[key] = value;
		}
	}

	const body = raw.slice(endIndex + 5); // skip past \n---\n
	return { meta, body };
}

/** Build YAML frontmatter string from metadata. */
function buildFrontmatter(meta: Record<string, string>): string {
	const lines = ["---"];
	for (const [key, value] of Object.entries(meta)) {
		lines.push(`${key}: ${value}`);
	}
	lines.push("---");
	return `${lines.join("\n")}\n`;
}

/**
 * Save a plan as a markdown file with YAML frontmatter.
 * Returns the file path.
 */
export async function savePlan(
	name: string,
	content: string,
	meta: { model?: string; sessionId?: string },
	projectPath?: string,
): Promise<string> {
	const plansDir = getPlansDir(projectPath);
	mkdirSync(plansDir, { recursive: true });

	const safeName = sanitizeName(name);
	const filePath = join(plansDir, `${safeName}.md`);

	const frontmatterData: Record<string, string> = {
		created: new Date().toISOString(),
	};
	if (meta.model) {
		frontmatterData.model = meta.model;
	}
	if (meta.sessionId) {
		frontmatterData.session_id = meta.sessionId;
	}

	const fileContent = `${buildFrontmatter(frontmatterData)}${content}\n`;
	writeFileSync(filePath, fileContent, "utf-8");

	return filePath;
}

/**
 * Load and parse a plan file. Returns null if not found.
 */
export async function loadPlan(
	name: string,
	projectPath?: string,
): Promise<{ name: string; content: string; meta: Record<string, string> } | null> {
	const plansDir = getPlansDir(projectPath);
	const safeName = sanitizeName(name);
	const filePath = join(plansDir, `${safeName}.md`);

	try {
		const raw = readFileSync(filePath, "utf-8");
		const { meta, body } = parseFrontmatter(raw);
		return { name: safeName, content: body.trim(), meta };
	} catch {
		return null;
	}
}

/**
 * List all plan files in the plans directory.
 * Returns name, created date, and first line as preview.
 */
export async function listPlans(
	projectPath?: string,
): Promise<Array<{ name: string; created: string; preview: string }>> {
	const plansDir = getPlansDir(projectPath);

	let files: string[];
	try {
		files = readdirSync(plansDir).filter((f) => f.endsWith(".md"));
	} catch {
		return [];
	}

	const results: Array<{ name: string; created: string; preview: string }> = [];

	for (const file of files) {
		const filePath = join(plansDir, file);
		try {
			const raw = readFileSync(filePath, "utf-8");
			const { meta, body } = parseFrontmatter(raw);
			const planName = basename(file, ".md");
			const firstLine = body.trim().split("\n")[0] ?? "";
			results.push({
				name: planName,
				created: meta.created ?? "",
				preview: firstLine,
			});
		} catch {
			// Skip unreadable files
		}
	}

	return results;
}
