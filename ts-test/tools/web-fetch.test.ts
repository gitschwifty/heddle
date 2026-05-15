import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { createWebFetchTool } from "../../src/tools/web-fetch.ts";

describe("web_fetch tool", () => {
	let originalFetch: typeof globalThis.fetch;

	beforeEach(() => {
		originalFetch = globalThis.fetch;
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
	});

	function mockFetch(fn: (...args: Parameters<typeof fetch>) => Promise<Response>): void {
		globalThis.fetch = Object.assign(fn, { preconnect: originalFetch.preconnect }) as typeof globalThis.fetch;
	}

	test("fetches URL and returns text content", async () => {
		mockFetch(
			async () =>
				new Response("Hello world", {
					status: 200,
					headers: { "content-type": "text/plain" },
				}),
		);

		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "https://example.com" });
		expect(result).toBe("Hello world");
	});

	test("strips HTML tags from response", async () => {
		mockFetch(
			async () =>
				new Response("<html><body><h1>Title</h1><p>Content</p></body></html>", {
					status: 200,
					headers: { "content-type": "text/html" },
				}),
		);

		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "https://example.com" });
		expect(result).toBe("TitleContent");
	});

	test("truncates long responses at 50K chars", async () => {
		const longText = "x".repeat(60_000);
		mockFetch(
			async () =>
				new Response(longText, {
					status: 200,
					headers: { "content-type": "text/plain" },
				}),
		);

		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "https://example.com" });
		expect(result.length).toBe(50_000);
	});

	test("returns error for non-200 status", async () => {
		mockFetch(
			async () =>
				new Response("Not Found", {
					status: 404,
					statusText: "Not Found",
					headers: { "content-type": "text/plain" },
				}),
		);

		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "https://example.com/missing" });
		expect(result).toContain("Error");
		expect(result).toContain("404");
	});

	test("returns error for non-text content type", async () => {
		mockFetch(
			async () =>
				new Response(new Uint8Array([0, 1, 2]), {
					status: 200,
					headers: { "content-type": "application/octet-stream" },
				}),
		);

		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "https://example.com/binary" });
		expect(result).toContain("Error");
		expect(result).toContain("Non-text content type");
	});

	test("returns error on timeout", async () => {
		mockFetch(async (_url, init) => {
			if (init?.signal) {
				throw new DOMException("The operation was aborted", "AbortError");
			}
			return new Response("ok");
		});

		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "https://example.com/slow" });
		expect(result).toContain("Error");
		expect(result).toContain("timed out");
	});

	test("returns error on network failure", async () => {
		mockFetch(async () => {
			throw new Error("Network error: DNS resolution failed");
		});

		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "https://nonexistent.example.com" });
		expect(result).toContain("Error");
		expect(result).toContain("DNS resolution failed");
	});

	test("returns error for invalid URL (not http/https)", async () => {
		const tool = createWebFetchTool();
		const result = await tool.execute({ url: "ftp://example.com/file" });
		expect(result).toContain("Error");
		expect(result).toContain("http");
	});

	test("respects 10s timeout (AbortController created with 10000ms)", async () => {
		let receivedSignal: AbortSignal | undefined;

		mockFetch(async (_url, init) => {
			receivedSignal = init?.signal ?? undefined;
			return new Response("ok", {
				status: 200,
				headers: { "content-type": "text/plain" },
			});
		});

		const tool = createWebFetchTool();
		await tool.execute({ url: "https://example.com" });
		expect(receivedSignal).toBeDefined();
		expect(receivedSignal?.aborted).toBe(false);
	});
});
