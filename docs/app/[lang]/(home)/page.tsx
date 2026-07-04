import { DynamicLink } from "fumadocs-core/dynamic-link";

export default function HomePage() {
	return (
		<div className="flex flex-col justify-center text-center flex-1">
			<h1 className="text-2xl font-bold mb-4">Hello World</h1>
			<p>
				You can open{" "}
				<DynamicLink href="/[lang]/docs" className="font-medium underline">
					/docs
				</DynamicLink>{" "}
				and see the documentation.
			</p>
		</div>
	);
}
