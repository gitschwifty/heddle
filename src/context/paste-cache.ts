import { createHash, randomBytes } from "node:crypto";

export interface CachedPaste {
	path: string;
	content: string;
	hash: string;
	timestamp: number;
	lines: number;
	pasteId?: string;
}

const DEFAULT_THRESHOLD = 10240; // 10KB

function generatePasteId(): string {
	return randomBytes(3).toString("hex");
}

function contentHash(content: string): string {
	return createHash("sha256").update(content).digest("hex");
}

export class PasteCache {
	private cache = new Map<string, CachedPaste>();
	private pasteIds = new Map<string, string>(); // pasteId → path
	private threshold: number;

	constructor(threshold?: number) {
		this.threshold = threshold ?? DEFAULT_THRESHOLD;
	}

	async resolve(absolutePath: string): Promise<CachedPaste> {
		const content = await Bun.file(absolutePath).text();
		const hash = contentHash(content);

		const existing = this.cache.get(absolutePath);
		if (existing && existing.hash === hash) {
			return existing;
		}

		// Remove old paste ID mapping if re-resolving a changed file
		if (existing?.pasteId) {
			this.pasteIds.delete(existing.pasteId);
		}

		const lines = content.split("\n").length;
		const byteLength = Buffer.byteLength(content, "utf-8");

		let pasteId: string | undefined;
		if (byteLength > this.threshold) {
			pasteId = generatePasteId();
			this.pasteIds.set(pasteId, absolutePath);
		}

		const entry: CachedPaste = {
			path: absolutePath,
			content,
			hash,
			timestamp: Date.now(),
			lines,
			pasteId,
		};

		this.cache.set(absolutePath, entry);
		return entry;
	}

	getByPasteId(id: string): CachedPaste | null {
		const path = this.pasteIds.get(id);
		if (!path) return null;
		return this.cache.get(path) ?? null;
	}

	list(): CachedPaste[] {
		return [...this.cache.values()];
	}

	clear(): void {
		this.cache.clear();
		this.pasteIds.clear();
	}

	get size(): number {
		return this.cache.size;
	}
}
