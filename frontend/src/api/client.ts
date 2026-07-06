// Tiny typed client for the `shbench serve` HTTP API (`/api/v1`).
//
// URLs are relative to the page origin. In development, Vite proxies `/api`
// (and `/healthz`) to `shbench serve` — see vite.config.ts. In production, serve
// the built `frontend/dist` from any static host and either put it behind a
// reverse proxy that forwards `/api` to `shbench serve`, or point it at the API
// via `VITE_API_PROXY`. (`shbench serve` itself is an API only — it does not
// serve the SPA.)

const API_BASE = "/api/v1";

/** The problem body shape returned by the API on error: `{ error: { code, message } }`. */
interface ApiErrorBody {
	code: string;
	message: string;
}

/**
 * Thrown when the API responds with a non-2xx status. Carries the error code.
 * Not exported: catch generically and render with {@link describeError}.
 */
class ApiError extends Error {
	readonly code: string;
	readonly status: number;

	constructor(code: string, message: string, status: number) {
		super(message);
		this.name = "ApiError";
		this.code = code;
		this.status = status;
	}
}

async function toApiError(res: Response): Promise<ApiError> {
	let code = "INTERNAL_ERROR";
	let message = res.statusText || `request failed (${res.status})`;
	try {
		const body = (await res.json()) as { error?: ApiErrorBody };
		if (body?.error) {
			code = body.error.code;
			message = body.error.message;
		}
	} catch {
		// Non-JSON error body — keep the status-derived defaults.
	}
	return new ApiError(code, message, res.status);
}

/**
 * Invoke an engine command by name. The request body is the command's typed
 * `Input`; the response is the bare typed `Output` (see the PRD output
 * contract). Throws {@link ApiError} on a non-2xx response.
 */
export async function callCommand<TOutput, TInput = unknown>(
	name: string,
	args: TInput = {} as TInput,
): Promise<TOutput> {
	const res = await fetch(`${API_BASE}/commands/${encodeURIComponent(name)}`, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify(args ?? {}),
	});
	if (!res.ok) throw await toApiError(res);
	return (await res.json()) as TOutput;
}

/** Format any thrown value for display, surfacing an {@link ApiError}'s code. */
export function describeError(err: unknown): string {
	if (err instanceof ApiError) return `${err.code}: ${err.message}`;
	if (err instanceof Error) return err.message;
	return String(err);
}
