export interface MatchResult {
	level: number; // 0=exact, 1=whitespace, 2=indent, 3=line-fuzzy
	startIndex: number; // position in original content
	matchedText: string; // the actual text that matched
}

/**
 * Try matching `search` against `content` using progressively fuzzier strategies.
 * Returns the first match found, or null if all levels fail.
 */
export function cascadingMatch(content: string, search: string): MatchResult | null {
	// Level 0: Exact match
	const exactIdx = content.indexOf(search);
	if (exactIdx !== -1) {
		return { level: 0, startIndex: exactIdx, matchedText: search };
	}

	// Level 1: Whitespace-normalized (preserve leading ws, collapse internal, trim trailing)
	const level1 = matchWhitespaceNormalized(content, search);
	if (level1) return level1;

	// Level 2: Indent-flexible (strip leading ws and compare)
	const level2 = matchIndentFlexible(content, search);
	if (level2) return level2;

	// Level 3: Line-by-line fuzzy (strip ALL whitespace per line)
	const level3 = matchLineFuzzy(content, search);
	if (level3) return level3;

	return null;
}

/**
 * Normalize a line: preserve leading whitespace, collapse internal whitespace
 * runs to single space, trim trailing whitespace.
 */
function normalizeLine(line: string): string {
	const leadingMatch = line.match(/^(\s*)/);
	const leading = leadingMatch ? leadingMatch[0] : "";
	const rest = line.slice(leading.length);
	return leading + rest.replace(/\s+/g, " ").trimEnd();
}

function matchWhitespaceNormalized(content: string, search: string): MatchResult | null {
	const contentLines = content.split("\n");
	const searchLines = search.split("\n");
	const normContentLines = contentLines.map(normalizeLine);
	const normSearchLines = searchLines.map(normalizeLine);
	const normContent = normContentLines.join("\n");
	const normSearch = normSearchLines.join("\n");

	const normIdx = normContent.indexOf(normSearch);
	if (normIdx === -1) return null;

	const matchStart = normIdx;
	const matchEnd = normIdx + normSearch.length;

	let pos = 0;
	let startLine = -1;
	let startCharInLine = -1;
	let endLine = -1;
	let endCharInLine = -1;

	for (let i = 0; i < normContentLines.length; i++) {
		const lineLen = normContentLines[i]!.length;
		const lineEnd = pos + lineLen;

		if (startLine === -1 && matchStart >= pos && matchStart <= lineEnd) {
			startLine = i;
			startCharInLine = matchStart - pos;
		}
		if (matchEnd >= pos && matchEnd <= lineEnd) {
			endLine = i;
			endCharInLine = matchEnd - pos;
			break;
		}

		pos = lineEnd + 1; // +1 for \n
	}

	if (startLine === -1 || endLine === -1) return null;

	const origStart = mapCharInLine(contentLines[startLine]!, normContentLines[startLine]!, startCharInLine, "start");
	const origEnd = mapCharInLine(contentLines[endLine]!, normContentLines[endLine]!, endCharInLine, "end");

	let absStart = 0;
	for (let i = 0; i < startLine; i++) {
		absStart += contentLines[i]!.length + 1;
	}
	absStart += origStart;

	let absEnd = 0;
	for (let i = 0; i < endLine; i++) {
		absEnd += contentLines[i]!.length + 1;
	}
	absEnd += origEnd;

	return {
		level: 1,
		startIndex: absStart,
		matchedText: content.slice(absStart, absEnd),
	};
}

/**
 * Map a character position in a normalized line back to the original line.
 */
function mapCharInLine(origLine: string, normLine: string, normCharPos: number, mode: "start" | "end"): number {
	if (mode === "end" && normCharPos >= normLine.length) {
		return origLine.length;
	}

	if (normCharPos === 0) return 0;

	const leading = origLine.match(/^(\s*)/)?.[0] ?? "";

	if (normCharPos <= leading.length) {
		return normCharPos;
	}

	let origPos = leading.length;
	let normPos = leading.length;

	while (normPos < normCharPos && origPos < origLine.length) {
		if (normLine[normPos] === " " && /\s/.test(origLine[origPos]!)) {
			normPos++;
			origPos++;
			while (origPos < origLine.length && /\s/.test(origLine[origPos]!)) {
				origPos++;
			}
		} else {
			normPos++;
			origPos++;
		}
	}

	return origPos;
}

function matchIndentFlexible(content: string, search: string): MatchResult | null {
	const contentLines = content.split("\n");
	const searchLines = search.split("\n");

	if (searchLines.length === 0) return null;

	const strippedSearch = searchLines.map((l) => l.trimStart());

	for (let i = 0; i <= contentLines.length - searchLines.length; i++) {
		let matches = true;
		for (let j = 0; j < searchLines.length; j++) {
			if (contentLines[i + j]!.trimStart() !== strippedSearch[j]) {
				matches = false;
				break;
			}
		}
		if (matches) {
			const matchedLines = contentLines.slice(i, i + searchLines.length);
			const matchedText = matchedLines.join("\n");

			let startIndex = 0;
			for (let k = 0; k < i; k++) {
				startIndex += contentLines[k]!.length + 1;
			}

			return { level: 2, startIndex, matchedText };
		}
	}

	return null;
}

function matchLineFuzzy(content: string, search: string): MatchResult | null {
	const contentLines = content.split("\n");
	const searchLines = search.split("\n");

	if (searchLines.length === 0) return null;

	const trimmedSearch = searchLines.map((l) => l.replace(/\s+/g, ""));

	for (let i = 0; i <= contentLines.length - searchLines.length; i++) {
		let matches = true;
		for (let j = 0; j < searchLines.length; j++) {
			if (contentLines[i + j]!.replace(/\s+/g, "") !== trimmedSearch[j]) {
				matches = false;
				break;
			}
		}
		if (matches) {
			const matchedLines = contentLines.slice(i, i + searchLines.length);
			const matchedText = matchedLines.join("\n");

			let startIndex = 0;
			for (let k = 0; k < i; k++) {
				startIndex += contentLines[k]!.length + 1;
			}

			return { level: 3, startIndex, matchedText };
		}
	}

	return null;
}

/**
 * When all match levels fail, find the closest matching location.
 */
export function findClosestMatch(content: string, search: string): { line: number; snippet: string } | null {
	const contentLines = content.split("\n");
	const searchLines = search.split("\n");
	const firstSearchLine = searchLines[0]!.trim().toLowerCase();

	if (firstSearchLine.length === 0) return null;

	const searchWords = firstSearchLine.split(/\s+/).filter(Boolean);
	if (searchWords.length === 0) return null;

	let bestLine = -1;
	let bestScore = 0;

	for (let i = 0; i < contentLines.length; i++) {
		const lineLower = contentLines[i]!.trim().toLowerCase();
		if (lineLower.length === 0) continue;

		let score = 0;
		for (const word of searchWords) {
			if (lineLower.includes(word)) {
				score += word.length;
			}
		}

		if (searchWords.length === 1) {
			const lcs = longestCommonSubstring(lineLower, firstSearchLine);
			score = Math.max(score, lcs);
		}

		if (score > bestScore) {
			bestScore = score;
			bestLine = i;
		}
	}

	if (bestLine === -1 || bestScore < 3) return null;

	const snippetStart = Math.max(0, bestLine - 1);
	const snippetEnd = Math.min(contentLines.length, bestLine + 2);
	const snippet = contentLines.slice(snippetStart, snippetEnd).join("\n");

	return { line: bestLine + 1, snippet };
}

function longestCommonSubstring(a: string, b: string): number {
	let best = 0;
	for (let i = 0; i < a.length; i++) {
		for (let j = 0; j < b.length; j++) {
			let len = 0;
			while (i + len < a.length && j + len < b.length && a[i + len] === b[j + len]) {
				len++;
			}
			if (len > best) best = len;
		}
	}
	return best;
}
