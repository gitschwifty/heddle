import { randomUUID } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

export interface MetaEntry {
	uuid: string;
	path: string;
	versions: number;
	previousPaths?: string[];
}

type MetaStore = Record<string, { path: string; versions: number; previousPaths?: string[] }>;

/**
 * Manages file-history metadata: maps UUIDs to file paths and version counts.
 * Persists to meta.json in the file-history base directory.
 */
export class FileHistoryMeta {
	private baseDir: string;
	private store: MetaStore;
	private loaded = false;

	constructor(baseDir: string) {
		this.baseDir = baseDir;
		this.store = {};
	}

	private load(): void {
		if (this.loaded) return;
		const metaPath = join(this.baseDir, "meta.json");
		if (existsSync(metaPath)) {
			try {
				this.store = JSON.parse(readFileSync(metaPath, "utf-8")) as MetaStore;
			} catch {
				this.store = {};
			}
		}
		this.loaded = true;
	}

	private async save(): Promise<void> {
		await mkdir(this.baseDir, { recursive: true });
		await writeFile(join(this.baseDir, "meta.json"), JSON.stringify(this.store, null, "\t"), "utf-8");
	}

	/** Find an entry by file path. Returns null if not tracked. */
	async findByPath(filePath: string): Promise<MetaEntry | null> {
		this.load();
		for (const [uuid, entry] of Object.entries(this.store)) {
			if (entry.path === filePath) {
				return { uuid, ...entry };
			}
		}
		return null;
	}

	/**
	 * Get or create a meta entry for a file path.
	 * If movedFromUuid is provided, the new entry records the old path in previousPaths.
	 */
	async getOrCreate(filePath: string, movedFromUuid?: string): Promise<MetaEntry> {
		this.load();

		// Check existing
		const existing = await this.findByPath(filePath);
		if (existing) return existing;

		// Create new entry
		const uuid = randomUUID();
		const entry: MetaStore[string] = { path: filePath, versions: 0 };

		// Track moved-from info
		if (movedFromUuid && this.store[movedFromUuid]) {
			const oldEntry = this.store[movedFromUuid];
			entry.previousPaths = [oldEntry.path];
		}

		this.store[uuid] = entry;
		await this.save();
		return { uuid, ...entry };
	}

	/** Increment the version count for a UUID and persist. */
	async incrementVersion(uuid: string): Promise<void> {
		this.load();
		const entry = this.store[uuid];
		if (!entry) return;
		entry.versions++;
		await this.save();
	}
}
