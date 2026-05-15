import { Type } from "@sinclair/typebox";
import type { HeddleTool } from "./types.ts";

const MAX_LENGTH = 50_000;

export function createWebFetchTool(): HeddleTool {
	return {
		name: "web_fetch",
		description: "Fetch the contents of a URL. Returns the text content with HTML tags stripped.",
		parameters: Type.Object({
			url: Type.String({ description: "The URL to fetch" }),
		}),
		execute: async (params) => {
			const { url } = params as { url: string };

			if (!url.startsWith("http://") && !url.startsWith("https://")) {
				return "Error: URL must start with http:// or https://";
			}

			try {
				const controller = new AbortController();
				const timeoutId = setTimeout(() => controller.abort(), 10_000);

				let response: Response;
				try {
					response = await fetch(url, { signal: controller.signal });
				} finally {
					clearTimeout(timeoutId);
				}

				if (!response.ok) {
					return `Error: HTTP ${response.status} ${response.statusText}`;
				}

				const contentType = response.headers.get("content-type") ?? "";
				if (!contentType.includes("text") && !contentType.includes("json") && !contentType.includes("xml")) {
					return `Error: Non-text content type: ${contentType}`;
				}

				let text = await response.text();
				text = text.replace(/<[^>]*>/g, "");

				if (text.length > MAX_LENGTH) {
					text = text.slice(0, MAX_LENGTH);
				}

				return text;
			} catch (err) {
				if (err instanceof DOMException && err.name === "AbortError") {
					return "Error: Request timed out after 10s";
				}
				return `Error: ${err instanceof Error ? err.message : String(err)}`;
			}
		},
	};
}
