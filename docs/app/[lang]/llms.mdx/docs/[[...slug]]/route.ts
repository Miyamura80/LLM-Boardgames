import { notFound } from "next/navigation";
import { getLLMText, source } from "@/lib/source";

export const revalidate = false;

export async function GET(
	_req: Request,
	{ params }: RouteContext<"/[lang]/llms.mdx/docs/[[...slug]]">,
) {
	const { slug, lang } = await params;
	const page = source.getPage(slug, lang);
	if (!page) notFound();

	return new Response(await getLLMText(page), {
		headers: {
			"Content-Type": "text/markdown",
		},
	});
}

export function generateStaticParams() {
	return source.generateParams();
}
