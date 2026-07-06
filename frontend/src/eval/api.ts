// Fetchers for the eval HTTP surface exposed by `shbench serve` (feature
// `store`). All paths are relative; Vite proxies `/api` to the server in dev.

import type { GameRecord, MatchOutcome } from "./types";

const API = "/api/v1";

async function getJson<T>(path: string): Promise<T> {
	const res = await fetch(`${API}${path}`);
	if (!res.ok) {
		const body = (await res.json().catch(() => ({}))) as { error?: string };
		throw new Error(body.error ?? `${res.status} ${res.statusText}`);
	}
	return (await res.json()) as T;
}

/** The most recent match: ratings, metrics, distribution, and its game records. */
export function fetchLeaderboard(): Promise<MatchOutcome> {
	return getJson<MatchOutcome>("/leaderboard");
}

export interface GameSummary {
	id: string;
	created_at: string;
	seed: number;
	winner: string;
	win_reason: string;
}

export function fetchGames(): Promise<GameSummary[]> {
	return getJson<GameSummary[]>("/games");
}

export function fetchGame(id: string): Promise<GameRecord> {
	return getJson<GameRecord>(`/games/${id}`);
}
