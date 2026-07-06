// Typed views of the Secret Hitler eval commands (`sh_*`).

import { callCommand } from "./client";

export interface RoleLine {
	mu: number;
	sigma: number;
	conservative: number;
	games: number;
	wins: number;
	win_rate: number;
}

export interface LeaderboardRow {
	model_id: string;
	is_anchor: boolean;
	liberal: RoleLine | null;
	fascist: RoleLine | null;
	hitler: RoleLine | null;
	overall_mu: number;
	overall_conservative: number;
	total_games: number;
	high_uncertainty: boolean;
}

export interface ModelMetricSummary {
	model_id: string;
	is_anchor: boolean;
	seats: number;
	suspicion_brier: number | null;
	liberal_suspicion_brier: number | null;
	goal_aligned_num: number;
	goal_aligned_den: number;
	throughput_num: number;
	throughput_den: number;
	exec_hits: number;
	exec_shots: number;
	malformed: number;
	illegal: number;
	forced: number;
	transport: number;
	prompt_tokens: number;
	completion_tokens: number;
}

export interface RunSummary {
	run_id: string;
	mode: string;
	status: string;
	games_played: number;
}

export interface GameSummary {
	game_id: string;
	run_id: string;
	schedule_label: string;
	winner: string;
	win_condition: string;
	rounds: number;
	duration_ms: number;
}

export type Role = "Liberal" | "Fascist" | "Hitler";

// GameEvent is an internally tagged union; we only need loose typing to render.
export interface GameEvent {
	type: string;
	[key: string]: unknown;
}

export interface EventRecord {
	idx: number;
	round: number;
	visibility: "Public" | { Private: number };
	event: GameEvent;
}

export interface RoleProbs {
	liberal: number;
	fascist: number;
	hitler: number;
}

export interface BeliefSnapshot {
	checkpoint: number;
	seat: number;
	report: { assessments: Record<string, RoleProbs> };
}

interface ThoughtRecord {
	round: number;
	at_event: number;
	decision: string;
	text: string;
}

export interface SeatRecord {
	seat: number;
	model_id: string;
	agent_kind: string;
	role: Role;
	is_anchor: boolean;
	survived: boolean;
	won: boolean;
	thoughts: ThoughtRecord[];
}

export interface GameRecord {
	game_id: string;
	seed: number;
	winner: string;
	win_condition: string;
	rounds: number;
	roles: Role[];
	seats: SeatRecord[];
	beliefs: BeliefSnapshot[];
	events: EventRecord[];
}

export const listRuns = () =>
	callCommand<{ runs: RunSummary[] }>("sh_list_runs");

export const fetchLeaderboard = (runId: string, includeAnchors: boolean) =>
	callCommand<{
		rows: LeaderboardRow[];
		metrics: ModelMetricSummary[];
		uncertainty_note: string;
	}>("sh_leaderboard", { run_id: runId, include_anchors: includeAnchors });

export const listGames = (runId: string) =>
	callCommand<{ games: GameSummary[] }>("sh_list_games", { run_id: runId });

export const fetchReplay = (gameId: string) =>
	callCommand<{ record: GameRecord; rendered: string[] }>("sh_game_replay", {
		game_id: gameId,
	});
