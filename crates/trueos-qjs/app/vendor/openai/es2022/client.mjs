/* Minimal OpenAI client for TRUEOS runtime.
 * Focus: robust `responses.create()` support with small surface area.
 */

import { readEnv } from "./internal/utils/env.mjs";
import {
	APIConnectionError,
	APIConnectionTimeoutError,
	APIError,
	OpenAIError,
} from "./core/error.mjs";

function debugLine(message) {
	const text = String(message ?? "");
	try {
		if (typeof globalThis.__trueosAiPrintLine === "function") {
			globalThis.__trueosAiPrintLine(text);
			return;
		}
		if (typeof console !== "undefined" && console && typeof console.log === "function") {
			console.log(text);
		}
	} catch {}
}

function isAbsoluteURL(url) {
	return /^https?:\/\//i.test(String(url || ""));
}

function encodeQueryValue(value) {
	return encodeURIComponent(String(value));
}

function appendQuery(url, query) {
	if (!query || typeof query !== "object") return url;
	const parts = [];
	for (const [key, value] of Object.entries(query)) {
		if (value === undefined || value === null) continue;
		parts.push(`${encodeQueryValue(key)}=${encodeQueryValue(value)}`);
	}
	if (parts.length === 0) return url;
	return `${url}${url.includes("?") ? "&" : "?"}${parts.join("&")}`;
}

function mergeHeaders(...items) {
	const out = Object.create(null);
	for (const item of items) {
		if (!item) continue;
		if (item instanceof Headers) {
			item.forEach((value, key) => {
				out[String(key).toLowerCase()] = String(value);
			});
			continue;
		}
		for (const [key, value] of Object.entries(item)) {
			if (value === undefined || value === null) continue;
			out[String(key).toLowerCase()] = String(value);
		}
	}
	return out;
}

function synthesizeOutputText(response) {
	if (!response || typeof response !== "object") return "";
	if (typeof response.output_text === "string") return response.output_text;
	const chunks = [];
	const output = Array.isArray(response.output) ? response.output : [];
	for (const item of output) {
		if (!item || item.type !== "message") continue;
		const content = Array.isArray(item.content) ? item.content : [];
		for (const part of content) {
			if (part && part.type === "output_text" && typeof part.text === "string") {
				chunks.push(part.text);
			}
		}
	}
	return chunks.join("");
}

class OpenAI {
	static DEFAULT_TIMEOUT = 120000;

	constructor({
		apiKey = readEnv("OPENAI_API_KEY"),
		baseURL = readEnv("OPENAI_BASE_URL") || "https://api.openai.com/v1",
		fetch: fetchImpl = globalThis.fetch,
		timeout = OpenAI.DEFAULT_TIMEOUT,
		defaultHeaders,
		defaultQuery,
	} = {}) {
		if (typeof fetchImpl !== "function") {
			throw new OpenAIError(
				"`fetch` is not defined as a global; provide `new OpenAI({ fetch })` or polyfill `globalThis.fetch`."
			);
		}

		this.fetch = fetchImpl;
		this.apiKey = apiKey;
		this.baseURL = String(baseURL || "https://api.openai.com/v1").replace(/\/+$/, "");
		this.timeout = Number(timeout) > 0 ? Number(timeout) : OpenAI.DEFAULT_TIMEOUT;
		this._options = {
			apiKey,
			baseURL: this.baseURL,
			fetch: fetchImpl,
			timeout: this.timeout,
			defaultHeaders,
			defaultQuery,
		};

		this.responses = {
			create: async (body, options = {}) => {
				if (!body || typeof body !== "object" || Array.isArray(body)) {
					throw new OpenAIError("Expected request body to be an object");
				}
				const data = await this.post("/responses", { body, ...options });
				if (typeof data.output_text !== "string") {
					data.output_text = synthesizeOutputText(data);
				}
				return data;
			},
		};
	}

	withOptions(options = {}) {
		return new OpenAI({ ...this._options, ...options });
	}

	defaultQuery() {
		return this._options.defaultQuery;
	}

	buildURL(path, query = undefined, defaultBaseURL = undefined) {
		const p = String(path || "");
		const root = defaultBaseURL || this.baseURL;
		const url = isAbsoluteURL(p)
			? p
			: `${root}${p.startsWith("/") ? "" : "/"}${p}`;

		const mergedQuery = {
			...(this.defaultQuery() || {}),
			...(query || {}),
		};
		return appendQuery(url, mergedQuery);
	}

	async authHeaders() {
		if (typeof this.apiKey === "string" && this.apiKey.length > 0) {
			return { Authorization: `Bearer ${this.apiKey}` };
		}
		return {};
	}

	async buildRequest(options, { retryCount = 0 } = {}) {
		const method = String(options.method || "GET").toUpperCase();
		const timeout = Number(options.timeout) > 0 ? Number(options.timeout) : this.timeout;
		const url = this.buildURL(options.path || "", options.query, options.defaultBaseURL);

		const headers = mergeHeaders(
			{
				Accept: "application/json",
				"Content-Type": "application/json",
				"X-Stainless-Retry-Count": String(retryCount),
			},
			await this.authHeaders(options),
			this._options.defaultHeaders,
			options.headers
		);

		const req = {
			method,
			headers,
			signal: options.signal,
		};

		if (options.body !== undefined) {
			req.body =
				typeof options.body === "string" ? options.body : JSON.stringify(options.body);
		}

		return { req, url, timeout };
	}

	async fetchWithTimeout(url, req, timeoutMs) {
		debugLine(`openai-client: fetch ${String(req && req.method || "GET")} ${String(url || "")}`);
		if (typeof AbortController !== "function") {
			return this.fetch(url, req);
		}

		const controller = new AbortController();
		const onAbort = () => controller.abort();
		if (req.signal && typeof req.signal.addEventListener === "function") {
			req.signal.addEventListener("abort", onAbort, { once: true });
		}

		const timer = setTimeout(() => controller.abort(), timeoutMs);
		try {
			return await this.fetch(url, { ...req, signal: controller.signal });
		} catch (err) {
			if (controller.signal.aborted) {
				throw new APIConnectionTimeoutError({ message: "Request timed out." });
			}
			throw new APIConnectionError({
				message: err && err.message ? err.message : "Connection error.",
				cause: err,
			});
		} finally {
			clearTimeout(timer);
		}
	}

	async request(options) {
		const { req, url, timeout } = await this.buildRequest(options);
		debugLine(`openai-client: request-built timeout=${String(timeout)} body=${req && typeof req.body === "string" ? String(req.body.length) : "0"}`);
		const response = await this.fetchWithTimeout(url, req, timeout);

		const text = await response.text();
		let parsed;
		try {
			parsed = text ? JSON.parse(text) : {};
		} catch {
			parsed = text;
		}

		if (!response.ok) {
			throw APIError.generate(response.status, parsed, text, response.headers);
		}

		return parsed;
	}

	get(path, options = {}) {
		return this.request({ ...options, method: "GET", path });
	}

	post(path, options = {}) {
		return this.request({ ...options, method: "POST", path });
	}

	patch(path, options = {}) {
		return this.request({ ...options, method: "PATCH", path });
	}

	put(path, options = {}) {
		return this.request({ ...options, method: "PUT", path });
	}

	delete(path, options = {}) {
		return this.request({ ...options, method: "DELETE", path });
	}
}

export { OpenAI };
export default OpenAI;
