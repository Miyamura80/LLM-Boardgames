import { docs } from "fumadocs-mdx:collections/server";
import { loader } from "fumadocs-core/source";
import { i18n } from "@/lib/i18n";

export const source = loader({
	baseUrl: "/docs",
	source: docs.toFumadocsSource(),
	i18n,
});

export function getPageImage(page: ReturnType<typeof source.getPage> & {}) {
	const slug = page.slugs;
	const segments = [...slug, "image.png"];

	return {
		url: `/${page.locale}/og/docs/${segments.join("/")}`,
		segments,
	};
}

export async function getLLMText(
	page: ReturnType<typeof source.getPages>[number],
) {
	const text = await page.data.getText("raw");
	return `# ${page.data.title}\n\n${page.data.description ?? ""}\n\nURL: ${page.url}\n\n${text}`;
}
