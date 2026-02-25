/**
 * Levenshtein distance and closest-match finder for tool name suggestions.
 */

export function levenshtein(a: string, b: string): number {
	const m = a.length;
	const n = b.length;

	// Use two rows instead of full matrix to avoid non-null assertions
	let prev = new Array<number>(n + 1);
	let curr = new Array<number>(n + 1);

	for (let j = 0; j <= n; j++) prev[j] = j;

	for (let i = 1; i <= m; i++) {
		curr[0] = i;
		for (let j = 1; j <= n; j++) {
			const cost = a[i - 1] === b[j - 1] ? 0 : 1;
			curr[j] = Math.min((prev[j] ?? 0) + 1, (curr[j - 1] ?? 0) + 1, (prev[j - 1] ?? 0) + cost);
		}
		[prev, curr] = [curr, prev];
	}

	return prev[n] ?? 0;
}

export function findClosest(query: string, candidates: string[], maxDistance = 3): string | null {
	let best: string | null = null;
	let bestDist = maxDistance + 1;

	for (const candidate of candidates) {
		const dist = levenshtein(query, candidate);
		if (dist < bestDist) {
			bestDist = dist;
			best = candidate;
		}
	}

	return bestDist <= maxDistance ? best : null;
}
