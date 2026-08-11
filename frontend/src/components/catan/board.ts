// Board geometry: reconstructs vertex/edge positions (and the engine's edge
// ids) from the BoardLaid event. The engine assigns edge ids in
// first-encounter order over hexes' clockwise corner pairs — mirrored exactly
// here so RoadBuilt events resolve without shipping an edge table.

import type { CatanEvent, HexSpec } from "../../api/catan";

export const HEX_SIZE = 46;

export interface Point {
	x: number;
	y: number;
}

export interface BoardGeometry {
	hexes: HexSpec[];
	hexCenter: Point[];
	vertexPos: Map<number, Point>;
	/** edge id → [vertexA, vertexB] (engine id order). */
	edgeEnds: Map<number, [number, number]>;
	width: number;
	height: number;
	/** Offset applied to all coordinates so the island sits in the viewBox. */
	offset: Point;
}

const SQRT3 = Math.sqrt(3);

/** Pointy-top hex corner offsets, clockwise from north (engine order). */
const CORNERS: Point[] = [
	{ x: 0, y: -1 },
	{ x: SQRT3 / 2, y: -0.5 },
	{ x: SQRT3 / 2, y: 0.5 },
	{ x: 0, y: 1 },
	{ x: -SQRT3 / 2, y: 0.5 },
	{ x: -SQRT3 / 2, y: -0.5 },
];

export function buildGeometry(board: CatanEvent): BoardGeometry {
	const hexes = board.hexes ?? [];
	const raw: Point[] = hexes.map((h) => ({
		x: HEX_SIZE * SQRT3 * (h.q + h.r / 2),
		y: HEX_SIZE * 1.5 * h.r,
	}));

	const vertexPos = new Map<number, Point>();
	const edgeEnds = new Map<number, [number, number]>();
	const edgeKeys = new Map<string, number>();
	for (const [i, h] of hexes.entries()) {
		for (let c = 0; c < 6; c++) {
			const v = h.vertices[c];
			if (!vertexPos.has(v)) {
				vertexPos.set(v, {
					x: raw[i].x + HEX_SIZE * CORNERS[c].x,
					y: raw[i].y + HEX_SIZE * CORNERS[c].y,
				});
			}
			const a = h.vertices[c];
			const b = h.vertices[(c + 1) % 6];
			const key = `${Math.min(a, b)}-${Math.max(a, b)}`;
			if (!edgeKeys.has(key)) {
				const id = edgeKeys.size;
				edgeKeys.set(key, id);
				edgeEnds.set(id, [Math.min(a, b), Math.max(a, b)]);
			}
		}
	}

	// Pad for the ocean frame.
	const xs = [...vertexPos.values()].map((p) => p.x);
	const ys = [...vertexPos.values()].map((p) => p.y);
	const pad = HEX_SIZE * 1.35;
	const minX = Math.min(...xs) - pad;
	const minY = Math.min(...ys) - pad;
	const offset = { x: -minX, y: -minY };
	for (const p of vertexPos.values()) {
		p.x += offset.x;
		p.y += offset.y;
	}
	const hexCenter = raw.map((p) => ({ x: p.x + offset.x, y: p.y + offset.y }));

	return {
		hexes,
		hexCenter,
		vertexPos,
		edgeEnds,
		width: Math.max(...xs) - Math.min(...xs) + 2 * pad,
		height: Math.max(...ys) - Math.min(...ys) + 2 * pad,
		offset,
	};
}

export function hexCornerPoints(center: Point): string {
	return CORNERS.map(
		(c) => `${center.x + HEX_SIZE * c.x},${center.y + HEX_SIZE * c.y}`,
	).join(" ");
}

/** Dice-roll probability weight of a number token (out of 36). */
export function pips(n: number): number {
	return 6 - Math.abs(7 - n);
}

/** Classic tabletop player colors: red, blue, orange, white. */
export const PLAYER_COLORS = ["#c0392b", "#2c5f8a", "#e07b1f", "#f2ede1"];
export const PLAYER_STROKES = ["#7d241a", "#1b3c58", "#94500f", "#8f8a7a"];

/** The board state folded from the event log up to (and including) a step. */
export interface FoldedBoard {
	buildings: Map<number, { owner: number; city: boolean }>;
	roads: Map<number, number>;
	robber: number;
	publicVp: number[];
	lastDice: [number, number] | null;
	longestRoad: number | null;
	largestArmy: number | null;
	turn: number;
	/** ids touched by the most recent board-changing event (for highlights). */
	lastVertex: number | null;
	lastEdge: number | null;
	lastHex: number | null;
}

export function foldBoard(events: { event: CatanEvent }[]): FoldedBoard {
	const f: FoldedBoard = {
		buildings: new Map(),
		roads: new Map(),
		robber: 0,
		publicVp: [0, 0, 0, 0],
		lastDice: null,
		longestRoad: null,
		largestArmy: null,
		turn: 0,
		lastVertex: null,
		lastEdge: null,
		lastHex: null,
	};
	for (const { event: e } of events) {
		switch (e.type) {
			case "BoardLaid":
				f.robber = e.desert ?? 0;
				break;
			case "TurnStarted":
				f.turn = e.turn ?? f.turn;
				break;
			case "SetupSettlementPlaced":
			case "SettlementBuilt":
				if (e.vertex !== undefined && e.seat !== undefined) {
					f.buildings.set(e.vertex, { owner: e.seat, city: false });
					f.publicVp[e.seat] += 1;
					f.lastVertex = e.vertex;
					f.lastEdge = null;
					f.lastHex = null;
				}
				break;
			case "CityBuilt":
				if (e.vertex !== undefined && e.seat !== undefined) {
					f.buildings.set(e.vertex, { owner: e.seat, city: true });
					f.publicVp[e.seat] += 1;
					f.lastVertex = e.vertex;
					f.lastEdge = null;
					f.lastHex = null;
				}
				break;
			case "SetupRoadPlaced":
			case "RoadBuilt":
				if (e.edge !== undefined && e.seat !== undefined) {
					f.roads.set(e.edge, e.seat);
					f.lastEdge = e.edge;
					f.lastVertex = null;
					f.lastHex = null;
				}
				break;
			case "RobberMoved":
				if (e.hex !== undefined) {
					f.robber = e.hex;
					f.lastHex = e.hex;
					f.lastVertex = null;
					f.lastEdge = null;
				}
				break;
			case "DiceRolled":
				if (e.d1 !== undefined && e.d2 !== undefined) {
					f.lastDice = [e.d1, e.d2];
				}
				break;
			case "LongestRoadClaimed": {
				const prev = f.longestRoad;
				if (prev !== null) f.publicVp[prev] -= 2;
				f.longestRoad = e.seat ?? null;
				if (f.longestRoad !== null) f.publicVp[f.longestRoad] += 2;
				break;
			}
			case "LargestArmyClaimed": {
				const prev = f.largestArmy;
				if (prev !== null) f.publicVp[prev] -= 2;
				if (e.seat !== undefined) {
					f.largestArmy = e.seat;
					f.publicVp[e.seat] += 2;
				}
				break;
			}
			default:
				break;
		}
	}
	return f;
}
