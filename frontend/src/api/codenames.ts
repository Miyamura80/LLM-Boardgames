// Hand-written TS mirrors of the Codenames engine types
// (crates/engine/src/codenames) plus thin wrappers over the generic command
// client. Field names and casings follow the serde attributes exactly:
//   Team / Role / EndReason  → snake_case ("a", "spymaster") / kebab-case
//   CardIdentity             → internally tagged on `kind`
//   CodenamesEvent           → internally tagged on `type` (variant names kept)

import { callCommand } from "./client";

export type Seat = number;

/** `a` always starts and always holds nine agents. */
export type Team = "a" | "b";

type Role = "spymaster" | "operative";

/** What a card really is, per the key card (`#[serde(tag = "kind")]`). */
export type CardIdentity =
	| { kind: "agent"; team: Team }
	| { kind: "bystander" }
	| { kind: "assassin" };

export type EndReason = "agents-found" | "assassin";

export interface Clue {
	/** Engine-normalized: trimmed and lowercased. */
	word: string;
	number: number;
}

type Visibility = "Public" | { Private: Seat };

/** The `type`-tagged transcript union. Every Codenames event is public. */
type CodenamesEvent =
	| {
			type: "GameStarted";
			starting_team: Team;
			wordlist_hash: string;
			rules_version: string;
	  }
	| { type: "BoardLaid"; words: string[] }
	| { type: "TurnStarted"; team: Team; turn: number }
	| { type: "ClueGiven"; seat: Seat; team: Team; clue: Clue }
	| {
			type: "GuessRevealed";
			seat: Seat;
			team: Team;
			word: string;
			identity: CardIdentity;
			ends_turn: boolean;
	  }
	| { type: "TurnPassed"; seat: Seat; team: Team }
	| { type: "ForcedDefault"; seat: Seat; decision: string }
	| { type: "GameEnded"; winner: Team; reason: EndReason; turns: number };

export interface EventRecord {
	idx: number;
	round: number;
	visibility: Visibility;
	event: CodenamesEvent;
}

interface ThoughtRecord {
	round: number;
	/** Transcript length when the decision resolved — one past its own event. */
	at_event: number;
	decision: string;
	text: string;
}

interface Reliability {
	malformed_outputs: number;
	illegal_moves: number;
	forced_defaults: number;
	transport_failures: number;
}

interface TokenUsage {
	prompt_tokens: number;
	completion_tokens: number;
}

export interface SeatRecord {
	/** 0 = A spymaster, 1 = A operative, 2 = B spymaster, 3 = B operative. */
	seat: Seat;
	role: Role;
	team: Team;
	model_id: string;
	agent_kind: string;
	scaffold_version: string;
	temperature: number | null;
	is_anchor: boolean;
	won: boolean;
	reliability: Reliability;
	thoughts: ThoughtRecord[];
	usage: TokenUsage;
}

export interface GameRecord {
	game_id: string;
	seed: number;
	rules_version: string;
	/** SHA-256 of the pool the grid was drawn from. */
	wordlist_hash: string;
	schedule_label: string;
	winner: Team;
	end_reason: EndReason;
	/** Team turns played (each turn = one clue plus its guesses). */
	turns: number;
	seats: SeatRecord[];
	events: EventRecord[];
	duration_ms: number;
}

/** One role's rating line; both are always reported. */
export interface RoleLine {
	mu: number;
	sigma: number;
	/** μ − kσ. */
	conservative: number;
	games: number;
	wins: number;
	win_rate: number;
}

/** Per-side win-rate diagnostic (never part of the rating). */
export interface SideLine {
	/** `starting` (team A) or `second`. */
	side: string;
	games: number;
	wins: number;
	win_rate: number;
}

export interface LeaderboardRow {
	model_id: string;
	is_anchor: boolean;
	spymaster: RoleLine | null;
	operative: RoleLine | null;
	overall_mu: number;
	overall_sigma: number;
	/** The headline: μ − kσ of the role-averaged numbers. */
	overall_conservative: number;
	total_games: number;
	sides: SideLine[];
	high_uncertainty: boolean;
}

export interface ModelMetricSummary {
	model_id: string;
	is_anchor: boolean;
	seats: number;
	wins: number;
	assassin_losses: number;
	clues_given: number;
	clue_number_sum: number;
	agents_found: number;
	clue_yield_den: number;
	enemy_hits_caused: number;
	bystander_hits_caused: number;
	assassin_hits_caused: number;
	guesses: number;
	guess_hits: number;
	first_guesses: number;
	first_guess_hits: number;
	bonus_guesses: number;
	bonus_misses: number;
	passes: number;
	pass_hazard_sum: number | null;
	assassin_hits: number;
	malformed: number;
	illegal: number;
	forced: number;
	transport: number;
	prompt_tokens: number;
	completion_tokens: number;
}

export interface RunSummary {
	run_id: string;
	/** Cross-game discriminator: always `codenames`. */
	game: string;
	mode: string;
	status: string;
	games_played: number;
	summary: unknown | null;
}

export interface GameSummary {
	game_id: string;
	run_id: string;
	schedule_label: string;
	winner: string;
	end_reason: string;
	turns: number;
	duration_ms: number;
}

export interface MatchProgress {
	run_id: string;
	total_games: number;
	played: number;
	remaining: number;
	prompt_tokens: number;
	completion_tokens: number;
}

interface PlayGameInput {
	models?: string[];
	set?: string;
	seed?: number;
	include_record?: boolean;
}

interface PlayGameOutput {
	game_id: string;
	seed: number;
	rules_version: string;
	wordlist_hash: string;
	winner: Team;
	end_reason: EndReason;
	turns: number;
	clues_given: number;
	cards_revealed: number;
	duration_ms: number;
	prompt_tokens: number;
	completion_tokens: number;
	forced_defaults: number;
	malformed_outputs: number;
	illegal_moves: number;
	events: number;
	record: GameRecord | null;
}

interface RunMatchInput {
	run_id: string;
	mode?: string;
	candidate?: string;
	pool?: string;
	boards?: number;
	k?: number;
	models?: string[];
	set?: string;
	games?: number;
	match_seed?: number;
	/** Play at most this many new games this call; `0` only reports progress. */
	max_games?: number;
}

export function codenamesListRuns(): Promise<{ runs: RunSummary[] }> {
	return callCommand("codenames_list_runs", {});
}

export function codenamesListGames(
	runId: string,
): Promise<{ games: GameSummary[] }> {
	return callCommand("codenames_list_games", { run_id: runId });
}

export function codenamesLeaderboard(runId: string): Promise<{
	run_id: string;
	rows: LeaderboardRow[];
	metrics: ModelMetricSummary[];
	uncertainty_note: string | null;
}> {
	return callCommand("codenames_leaderboard", { run_id: runId });
}

export function codenamesReplay(
	gameId: string,
): Promise<{ record: GameRecord; rendered: string[] }> {
	return callCommand("codenames_game_replay", { game_id: gameId });
}

/**
 * Play one ad-hoc 4-seat game. Needs no database — with the default
 * `bot:codenames-random` seats it is a free, local way to get a live record
 * into the replay view.
 */
export function codenamesPlayGame(
	input: PlayGameInput,
): Promise<PlayGameOutput> {
	return callCommand("codenames_play_game", input);
}

/**
 * Run or resume a stored match. The console calls this with `max_games: 0`,
 * which plays nothing and just reports the run's schedule progress.
 */
export function codenamesRunMatch(input: RunMatchInput): Promise<{
	run_id: string;
	progress: MatchProgress;
	leaderboard: LeaderboardRow[] | null;
	metrics: ModelMetricSummary[] | null;
	uncertainty_note: string | null;
}> {
	return callCommand("codenames_run_match", input);
}
