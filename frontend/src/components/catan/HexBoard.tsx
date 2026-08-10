// The tabletop-faithful SVG board: illustrated terrain hexes, pip number
// tokens, coastal ports, colored piece silhouettes, and the robber. Original
// artwork evoking the wooden tabletop game (no trademark assets) —
// see PRD-catan-evals US-C13.

import type { CatanEvent, Terrain } from "../../api/catan";
import type { BoardGeometry, FoldedBoard, Point } from "./board";
import {
	HEX_SIZE,
	hexCornerPoints,
	PLAYER_COLORS,
	PLAYER_STROKES,
	pips,
} from "./board";

const TERRAIN_FILL: Record<Terrain, [string, string]> = {
	fields: ["#e8c95a", "#d4a92f"],
	forest: ["#3f7d3c", "#2a5c28"],
	pasture: ["#a3c86a", "#7ea94b"],
	hills: ["#c96f42", "#a9542d"],
	mountains: ["#9aa1ab", "#767e8b"],
	desert: ["#e3d3a2", "#d0bc82"],
};

function TerrainDecor({ t, c }: { t: Terrain; c: Point }) {
	const s = HEX_SIZE;
	switch (t) {
		case "forest":
			return (
				<g fill="#1d4520" opacity={0.85}>
					{[
						[-0.42, 0.1],
						[0.28, -0.3],
						[0.05, 0.42],
					].map(([dx, dy]) => (
						<path
							key={`${dx}`}
							d={`M ${c.x + dx * s} ${c.y + dy * s}
							   l ${s * 0.14} ${s * 0.3} h ${-s * 0.28} Z
							   M ${c.x + dx * s} ${c.y + (dy - 0.16) * s}
							   l ${s * 0.11} ${s * 0.24} h ${-s * 0.22} Z`}
						/>
					))}
				</g>
			);
		case "hills":
			return (
				<g fill="#8a3f1d" opacity={0.75}>
					{[
						[-0.45, 0.05],
						[-0.12, 0.05],
						[0.21, 0.05],
						[-0.28, 0.24],
						[0.05, 0.24],
					].map(([dx, dy]) => (
						<rect
							key={`${dx}-${dy}`}
							x={c.x + dx * s}
							y={c.y + dy * s}
							width={s * 0.28}
							height={s * 0.14}
							rx={s * 0.03}
						/>
					))}
				</g>
			);
		case "mountains":
			return (
				<g>
					<path
						d={`M ${c.x - 0.5 * s} ${c.y + 0.35 * s} l ${0.3 * s} ${-0.6 * s} l ${0.25 * s} ${0.6 * s} Z`}
						fill="#5d6570"
					/>
					<path
						d={`M ${c.x - 0.05 * s} ${c.y + 0.35 * s} l ${0.28 * s} ${-0.75 * s} l ${0.3 * s} ${0.75 * s} Z`}
						fill="#6f7885"
					/>
					<path
						d={`M ${c.x + 0.16 * s} ${c.y - 0.18 * s} l ${0.07 * s} ${-0.22 * s} l ${0.12 * s} ${0.3 * s} Z`}
						fill="#e8ecf2"
					/>
				</g>
			);
		case "fields":
			return (
				<g stroke="#a87f14" strokeWidth={1.6} opacity={0.8}>
					{[-0.32, -0.12, 0.08, 0.28].map((dy) => (
						<path
							key={dy}
							fill="none"
							d={`M ${c.x - 0.5 * s} ${c.y + dy * s} q ${0.25 * s} ${-0.12 * s} ${0.5 * s} 0 q ${0.25 * s} ${0.12 * s} ${0.5 * s} 0`}
						/>
					))}
				</g>
			);
		case "pasture":
			return (
				<g fill="#f5f2e6" stroke="#c6c0a8" strokeWidth={0.8}>
					{[
						[-0.3, -0.05],
						[0.22, 0.18],
						[-0.05, 0.36],
					].map(([dx, dy]) => (
						<g key={`${dx}`}>
							<ellipse
								cx={c.x + dx * s}
								cy={c.y + dy * s}
								rx={s * 0.13}
								ry={s * 0.09}
							/>
							<circle
								cx={c.x + (dx - 0.11) * s}
								cy={c.y + (dy - 0.03) * s}
								r={s * 0.05}
								fill="#d8d2bd"
							/>
						</g>
					))}
				</g>
			);
		case "desert":
			return (
				<g stroke="#bfa86a" strokeWidth={1.4} opacity={0.7} fill="none">
					{[-0.15, 0.1, 0.32].map((dy) => (
						<path
							key={dy}
							d={`M ${c.x - 0.4 * s} ${c.y + dy * s} q ${0.2 * s} ${-0.1 * s} ${0.4 * s} 0 t ${0.4 * s} 0`}
						/>
					))}
				</g>
			);
	}
}

function NumberToken({ n, c }: { n: number; c: Point }) {
	const hot = n === 6 || n === 8;
	const r = HEX_SIZE * 0.31;
	return (
		<g>
			<circle
				cx={c.x}
				cy={c.y}
				r={r}
				fill="#f7f0dc"
				stroke="#b3a684"
				strokeWidth={1.5}
			/>
			<text
				x={c.x}
				y={c.y + r * 0.18}
				textAnchor="middle"
				fontSize={r * (hot ? 1.15 : 1.0)}
				fontWeight={hot ? 800 : 600}
				fill={hot ? "#c0392b" : "#3b3325"}
				fontFamily="Georgia, serif"
			>
				{n}
			</text>
			<g fill={hot ? "#c0392b" : "#6b604a"}>
				{Array.from({ length: pips(n) }, (_, i) => (
					<circle
						// biome-ignore lint/suspicious/noArrayIndexKey: fixed-order dots
						key={i}
						cx={c.x + (i - (pips(n) - 1) / 2) * (r * 0.3)}
						cy={c.y + r * 0.55}
						r={r * 0.07}
					/>
				))}
			</g>
		</g>
	);
}

function Robber({ c }: { c: Point }) {
	const s = HEX_SIZE * 0.36;
	return (
		<g
			transform={`translate(${c.x + HEX_SIZE * 0.42}, ${c.y - HEX_SIZE * 0.1})`}
		>
			<ellipse
				cx={0}
				cy={s * 0.75}
				rx={s * 0.55}
				ry={s * 0.18}
				fill="#00000055"
			/>
			<path
				d={`M ${-s * 0.45} ${s * 0.7} q 0 ${-s * 0.7} ${s * 0.2} ${-s * 0.9}
				   a ${s * 0.28} ${s * 0.28} 0 1 1 ${s * 0.5} 0
				   q ${s * 0.2} ${s * 0.2} ${s * 0.2} ${s * 0.9} Z`}
				fill="#2b2b33"
				stroke="#101014"
				strokeWidth={1.2}
			/>
		</g>
	);
}

function Settlement({ p, owner }: { p: Point; owner: number }) {
	const s = HEX_SIZE * 0.24;
	return (
		<path
			d={`M ${p.x - s} ${p.y + s * 0.85} v ${-s} l ${s} ${-s * 0.8} l ${s} ${s * 0.8} v ${s} Z`}
			fill={PLAYER_COLORS[owner]}
			stroke={PLAYER_STROKES[owner]}
			strokeWidth={2}
			strokeLinejoin="round"
		/>
	);
}

function City({ p, owner }: { p: Point; owner: number }) {
	const s = HEX_SIZE * 0.3;
	return (
		<path
			d={`M ${p.x - s} ${p.y + s * 0.8} v ${-s * 1.1} l ${s * 0.5} ${-s * 0.55} l ${s * 0.5} ${s * 0.55}
			   v ${s * 0.35} h ${s} v ${s * 0.75} Z`}
			fill={PLAYER_COLORS[owner]}
			stroke={PLAYER_STROKES[owner]}
			strokeWidth={2}
			strokeLinejoin="round"
		/>
	);
}

export function HexBoard({
	geometry,
	board,
	fold,
}: {
	geometry: BoardGeometry;
	board: CatanEvent;
	fold: FoldedBoard;
}) {
	const g = geometry;
	const center: Point = { x: g.width / 2, y: g.height / 2 };
	const portOut = (a: Point, b: Point): Point => {
		const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
		const dx = mid.x - center.x;
		const dy = mid.y - center.y;
		const len = Math.hypot(dx, dy) || 1;
		return {
			x: mid.x + (dx / len) * HEX_SIZE * 0.62,
			y: mid.y + (dy / len) * HEX_SIZE * 0.62,
		};
	};

	return (
		<svg
			viewBox={`0 0 ${g.width} ${g.height}`}
			className="catan-board"
			role="img"
			aria-label="Catan board"
		>
			<defs>
				{Object.entries(TERRAIN_FILL).map(([t, [light, dark]]) => (
					<radialGradient key={t} id={`terrain-${t}`} cx="50%" cy="42%" r="75%">
						<stop offset="0%" stopColor={light} />
						<stop offset="100%" stopColor={dark} />
					</radialGradient>
				))}
				<radialGradient id="ocean" cx="50%" cy="50%" r="72%">
					<stop offset="0%" stopColor="#2e7396" />
					<stop offset="100%" stopColor="#1d4d68" />
				</radialGradient>
			</defs>

			<rect width={g.width} height={g.height} fill="url(#ocean)" rx={14} />
			{[1.12, 1.24].map((k) => (
				<ellipse
					key={k}
					cx={center.x}
					cy={center.y}
					rx={(g.width / 2.4) * k}
					ry={(g.height / 2.55) * k}
					fill="none"
					stroke="#ffffff18"
					strokeWidth={2}
				/>
			))}

			{g.hexes.map((h, i) => (
				<g key={h.hex}>
					<polygon
						points={hexCornerPoints(g.hexCenter[i])}
						fill={`url(#terrain-${h.terrain})`}
						stroke="#e8dcbd"
						strokeWidth={2.6}
						strokeLinejoin="round"
						className={fold.lastHex === h.hex ? "catan-flash" : undefined}
					/>
					<TerrainDecor t={h.terrain} c={g.hexCenter[i]} />
					{h.number !== null && <NumberToken n={h.number} c={g.hexCenter[i]} />}
				</g>
			))}

			{(board.ports ?? []).map((p) => {
				const a = g.vertexPos.get(p.vertices[0]);
				const b = g.vertexPos.get(p.vertices[1]);
				if (!a || !b) return null;
				const at = portOut(a, b);
				const label =
					p.port.kind === "generic" ? "3:1" : `2:1 ${p.port.resource}`;
				return (
					<g key={p.edge}>
						<line
							x1={a.x}
							y1={a.y}
							x2={at.x}
							y2={at.y}
							stroke="#c9a86255"
							strokeWidth={3}
						/>
						<line
							x1={b.x}
							y1={b.y}
							x2={at.x}
							y2={at.y}
							stroke="#c9a86255"
							strokeWidth={3}
						/>
						<path
							d={`M ${at.x - 10} ${at.y} q 10 9 20 0 l -3 -4 h -14 Z M ${at.x} ${at.y - 2} v -9 l 8 7 Z`}
							fill="#e8dcbd"
							stroke="#8a7448"
							strokeWidth={1}
						/>
						<text
							x={at.x}
							y={at.y + 16}
							textAnchor="middle"
							fontSize={10.5}
							fontWeight={700}
							fill="#f3ead2"
							fontFamily="Georgia, serif"
						>
							{label}
						</text>
					</g>
				);
			})}

			{[...fold.roads.entries()].map(([edge, owner]) => {
				const ends = g.edgeEnds.get(edge);
				if (!ends) return null;
				const a = g.vertexPos.get(ends[0]);
				const b = g.vertexPos.get(ends[1]);
				if (!a || !b) return null;
				// Inset the segment so roads read as pieces, not borders.
				const t = 0.18;
				const ax = a.x + (b.x - a.x) * t;
				const ay = a.y + (b.y - a.y) * t;
				const bx = b.x - (b.x - a.x) * t;
				const by = b.y - (b.y - a.y) * t;
				return (
					<g
						key={edge}
						className={fold.lastEdge === edge ? "catan-flash" : undefined}
					>
						<line
							x1={ax}
							y1={ay}
							x2={bx}
							y2={by}
							stroke={PLAYER_STROKES[owner]}
							strokeWidth={9}
							strokeLinecap="round"
						/>
						<line
							x1={ax}
							y1={ay}
							x2={bx}
							y2={by}
							stroke={PLAYER_COLORS[owner]}
							strokeWidth={5.5}
							strokeLinecap="round"
						/>
					</g>
				);
			})}

			{[...fold.buildings.entries()].map(([vertex, b]) => {
				const p = g.vertexPos.get(vertex);
				if (!p) return null;
				return (
					<g
						key={vertex}
						className={fold.lastVertex === vertex ? "catan-flash" : undefined}
					>
						{b.city ? (
							<City p={p} owner={b.owner} />
						) : (
							<Settlement p={p} owner={b.owner} />
						)}
					</g>
				);
			})}

			<Robber c={g.hexCenter[fold.robber] ?? center} />
		</svg>
	);
}
