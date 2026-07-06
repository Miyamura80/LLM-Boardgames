use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use app_config as config;
use base64::{engine::general_purpose, Engine as _};
use clap::{Parser, Subcommand};
use image::codecs::ico::IcoEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::{invert, resize, FilterType};
use image::ImageEncoder;
use image::{ColorType, DynamicImage, GenericImage, ImageBuffer, Rgba, RgbaImage};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{error, info, warn};

/// Minimal standalone logging for this dev tool (the engine/server crates own
/// their own logging setup).
fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .try_init();
}

const IMAGE_MODEL: &str = "gemini-3-pro-image-preview";
const IMAGE_PROMPT_STYLE: &str = "Create a minimalist, modern horizontal wordmark logo (4:1 aspect) with an icon on the left and clear text on the right. Use dark tones, clean typography, and avoid photorealism. The background should be bright lime green (#00FF00) to act as a greenscreen, but keep the logo colors distinct and readable.";
const ICON_EXTRACTION_PROMPT: &str = "Remove ALL TEXT from this image. Keep ONLY the icon/symbol from the left side, center it in a square 1:1 aspect ratio, and preserve the BRIGHT LIME GREEN (#00FF00) background exactly as it appears. Do not tweak the icon colors, just remove the text and center the symbol.";
const BANNER_STYLE_PROMPT: &str = "Style the image in a Japanese minimalist sumi-e ink wash style with monochrome tones, fluid brushstrokes, and thoughtful negative space. Use a wide 16:9 composition, keep the view horizontal, and make the banner the dominant focal point with legible text centered at the top.";

#[derive(Parser)]
#[command(author, version, about = "Legacy asset generator replacement", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate the project logo, icons, and favicon
    Logo {
        /// Project name used in prompts
        #[arg(long)]
        project_name: Option<String>,
        /// Optional creative suggestion for the wordmark
        #[arg(long)]
        suggestion: Option<String>,
        /// Where to write assets (defaults to docs/public)
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Generate the hero banner image
    Banner {
        /// Title/text that belongs on the banner
        #[arg(long)]
        title: Option<String>,
        /// Optional guiding suggestion for the description
        #[arg(long)]
        suggestion: Option<String>,
        /// Output directory for the banner (defaults to media/)
        #[arg(long)]
        output_dir: Option<PathBuf>,
        /// Path to an icon/logo image to incorporate into the banner.
        /// If omitted, falls back to docs/public/icon-light.png when it exists.
        #[arg(long)]
        icon: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install ring as the rustls crypto provider (reqwest needs this with rustls-no-provider)
    let _ = rustls::crypto::ring::default_provider().install_default();

    init_logging();
    let cli = Cli::parse();
    let client = GeminiClient::new()?;

    match cli.command {
        Command::Logo {
            project_name,
            suggestion,
            output_dir,
        } => run_logo(project_name, suggestion, output_dir, client).await,
        Command::Banner {
            title,
            suggestion,
            output_dir,
            icon,
        } => run_banner(title, suggestion, output_dir, icon, client).await,
    }
}

async fn run_logo(
    project_name: Option<String>,
    suggestion: Option<String>,
    output_dir: Option<PathBuf>,
    client: GeminiClient,
) -> Result<()> {
    let workspace = workspace_root()?;
    let project_name = match project_name {
        Some(name) => name,
        None => read_project_name(&workspace)
            .await
            .unwrap_or_else(|_| "Secret-Hitler-Evals".into()),
    };
    let target = output_dir.unwrap_or_else(|| workspace.join("docs").join("public"));
    tokio::fs::create_dir_all(&target)
        .await
        .context("Failed to create output directory")?;

    info!("Generating wordmark for {}...", project_name);
    let description = client
        .generate_text_description(&project_name, suggestion.as_deref())
        .await
        .context("Failed to describe the wordmark")?;

    let prompt = format!(
        "{description}. Create a HORIZONTAL 4:1 wordmark logo (3200x800) that includes the text '{project_name}'. {IMAGE_PROMPT_STYLE} Use DARK colors to match a light mode header, keep the icon on the left, and ensure the lime-green background exists only to support chroma-keying.",
    );

    let mut light_image = client
        .generate_image(IMAGE_MODEL, &prompt)
        .await
        .context("Failed to generate light mode wordmark")?
        .to_rgba8();
    let icon_reference = light_image.clone();
    info!("Extracting icon from wordmark...");
    let icon_prompt = format!(
        "{ICON_EXTRACTION_PROMPT} Remove the text '{project_name}' and keep only the icon."
    );
    let mut icon_light = client
        .generate_image_from_reference(IMAGE_MODEL, &icon_prompt, &icon_reference)
        .await
        .context("Failed to extract icon")?
        .to_rgba8();

    remove_greenscreen(&mut light_image, 60);
    save_png(&light_image, &target.join("logo-light.png"))?;
    info!(
        "Saved light wordmark at {}",
        target.join("logo-light.png").display()
    );
    remove_greenscreen(&mut icon_light, 60);

    let mut dark_wordmark = light_image.clone();
    invert(&mut dark_wordmark);
    save_png(&dark_wordmark, &target.join("logo-dark.png"))?;
    info!(
        "Saved dark wordmark at {}",
        target.join("logo-dark.png").display()
    );

    let mut icon_dark = icon_light.clone();
    invert(&mut icon_dark);

    let icon_light_square = ensure_square(&icon_light)?;
    let icon_dark_square = ensure_square(&icon_dark)?;

    let icon_light_512 = resize(&icon_light_square, 512, 512, FilterType::Lanczos3);
    let icon_dark_512 = resize(&icon_dark_square, 512, 512, FilterType::Lanczos3);
    let favicon_32 = resize(&icon_light_square, 32, 32, FilterType::Lanczos3);

    save_png(&icon_light_512, &target.join("icon-light.png"))?;
    save_png(&icon_dark_512, &target.join("icon-dark.png"))?;
    save_ico(&favicon_32, &target.join("favicon.ico"))?;

    // Also emit a 1024x1024 source icon (useful for docs / social cards).
    let icon_1024 = resize(&icon_light_square, 1024, 1024, FilterType::Lanczos3);
    save_png(&icon_1024, &target.join("icon-1024.png"))?;

    info!("Logo assets saved to {}", target.display());
    Ok(())
}

async fn run_banner(
    title: Option<String>,
    suggestion: Option<String>,
    output_dir: Option<PathBuf>,
    icon: Option<PathBuf>,
    client: GeminiClient,
) -> Result<()> {
    let workspace = workspace_root()?;
    let title = match title {
        Some(t) => t,
        None => read_project_name(&workspace)
            .await
            .unwrap_or_else(|_| "Secret-Hitler-Evals".into()),
    };
    let target = output_dir.unwrap_or_else(|| workspace.join("media"));
    tokio::fs::create_dir_all(&target)
        .await
        .context("Failed to create banner output directory")?;

    // Try to load an icon image: explicit --icon flag, or fall back to docs/public/icon-light.png
    let explicit_icon = icon.is_some();
    let icon_path = icon.or_else(|| {
        let default = workspace.join("docs").join("public").join("icon-light.png");
        default.exists().then_some(default)
    });
    let icon_image = match &icon_path {
        Some(p) => {
            info!("Using icon from {}", p.display());
            match image::open(p) {
                Ok(img) => Some(img.to_rgba8()),
                Err(e) if !explicit_icon => {
                    // Auto-detected path failed — degrade gracefully
                    warn!(
                        "Failed to load auto-detected icon at {}: {e}, continuing without reference",
                        p.display()
                    );
                    None
                }
                Err(e) => {
                    return Err(anyhow::Error::from(e))
                        .with_context(|| format!("Failed to load icon at {}", p.display()));
                }
            }
        }
        None => {
            info!("No icon found, generating banner without logo reference");
            None
        }
    };

    let banner_description = client
        .generate_banner_description(&title, suggestion.as_deref())
        .await
        .context("Failed to describe banner")?;

    let banner = if let Some(ref icon_img) = icon_image {
        let full_prompt = format!(
            "{banner_description}. Create a WIDE 16:9 horizontal image where the banner takes up 80% of the screen and the text '{title}' is centered at the top with excellent contrast. {BANNER_STYLE_PROMPT} IMPORTANT: Use the provided icon/logo as the main visual element in the banner — do NOT use a default placeholder icon. Incorporate this exact icon prominently in the composition.",
        );
        client
            .generate_image_from_reference(IMAGE_MODEL, &full_prompt, icon_img)
            .await
            .context("Failed to generate banner with icon reference")?
    } else {
        let full_prompt = format!(
            "{banner_description}. Create a WIDE 16:9 horizontal image where the banner takes up 80% of the screen and the text '{title}' is centered at the top with excellent contrast. {BANNER_STYLE_PROMPT}",
        );
        client
            .generate_image(IMAGE_MODEL, &full_prompt)
            .await
            .context("Failed to generate banner")?
    };

    let banner_path = target.join("banner.png");
    banner
        .save(&banner_path)
        .context("Failed to write banner image")?;

    info!("Banner saved to {}", banner_path.display());
    Ok(())
}

fn save_png(image: &RgbaImage, path: &Path) -> Result<()> {
    image
        .save(path)
        .with_context(|| format!("Failed to save PNG at {}", path.display()))
}

fn save_ico(image: &RgbaImage, path: &Path) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("Failed to open ICO file at {}", path.display()))?;
    let encoder = IcoEncoder::new(file);
    encoder
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::Rgba8.into(),
        )
        .with_context(|| format!("Failed to write ICO at {}", path.display()))
}

fn remove_greenscreen(image: &mut RgbaImage, tolerance: i32) {
    for pixel in image.pixels_mut() {
        let [r, mut g, b, mut a] = pixel.0;
        let tolerance_f = tolerance as f32;
        let r_f = r as f32;
        let g_f = g as f32;
        let b_f = b as f32;

        let green_high = g_f > 180.0;
        let green_dominant = g_f > r_f + tolerance_f + 20.0 && g_f > b_f + tolerance_f + 20.0;
        if green_high && green_dominant {
            a = 0;
        }

        let visible = a > 128;
        let has_green_tint = g_f > r_f + 20.0 && g_f > b_f + 20.0;
        if visible && has_green_tint {
            let avg_rb = (r_f + b_f) / 2.0;
            let new_g = (g_f * 0.6).min(avg_rb);
            g = new_g.clamp(0.0, 255.0) as u8;
        }

        pixel.0 = [r, g, b, a];
    }
}

fn ensure_square(image: &RgbaImage) -> Result<RgbaImage> {
    let size = image.width().max(image.height());
    let mut square = ImageBuffer::from_pixel(size, size, Rgba([255, 255, 255, 0]));
    let offset_x = (size - image.width()) / 2;
    let offset_y = (size - image.height()) / 2;
    square
        .copy_from(image, offset_x, offset_y)
        .with_context(|| "Failed to center image in square canvas")?;
    Ok(square)
}

fn workspace_root() -> Result<PathBuf> {
    // This crate lives at `<workspace>/crates/assetgen`, so the workspace root
    // is two directories up from the manifest dir.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("Unable to determine workspace root"))
}

async fn read_project_name(workspace: &Path) -> Result<String> {
    let package_json = workspace.join("package.json");
    let data = tokio::fs::read_to_string(&package_json)
        .await
        .with_context(|| format!("Failed to read {}", package_json.display()))?;
    let json: Value = serde_json::from_str(&data).context("Invalid package.json")?;
    json.get("name")
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("package.json does not declare a name"))
}

struct GeminiClient {
    http: Client,
    api_key: String,
    text_model: String,
}

impl GeminiClient {
    fn new() -> Result<Self> {
        let cfg = config::get_config();
        let api_key = cfg
            .gemini_api_key()
            .ok_or_else(|| anyhow!("Missing APP__GEMINI_API_KEY"))?
            .to_string();
        // Strip provider prefix (e.g. "gemini/gemini-3-flash-preview" -> "gemini-3-flash-preview")
        let text_model = cfg
            .model_name
            .rsplit_once('/')
            .map(|(_, name): (&str, &str)| name.to_string())
            .unwrap_or_else(|| cfg.model_name.clone());
        Ok(Self {
            http: Client::new(),
            api_key,
            text_model,
        })
    }

    async fn generate_text_description(
        &self,
        title: &str,
        suggestion: Option<&str>,
    ) -> Result<String> {
        let prompt = format!(
            "Create a concise, creative description of a modern horizontal wordmark for '{title}'. {}",
            suggestion.unwrap_or(""),
        );
        self.generate_text(&self.text_model, &prompt).await
    }

    async fn generate_banner_description(
        &self,
        title: &str,
        suggestion: Option<&str>,
    ) -> Result<String> {
        let prompt = format!(
            "Describe a Japanese-style banner featuring the text '{title}'. {}",
            suggestion.unwrap_or(""),
        );
        self.generate_text(&self.text_model, &prompt).await
    }

    async fn generate_text(&self, model: &str, prompt: &str) -> Result<String> {
        let request = GenerateContentRequest::new_text(prompt);
        let response = self.send_request(model, &request).await?;
        extract_text(&response).ok_or_else(|| anyhow!("No text returned from Gemini"))
    }

    async fn generate_image(&self, model: &str, prompt: &str) -> Result<DynamicImage> {
        let request = GenerateContentRequest::new_image(prompt);
        let response = self.send_request(model, &request).await?;
        extract_first_image(&response).ok_or_else(|| anyhow!("No image returned from Gemini"))
    }

    async fn generate_image_from_reference(
        &self,
        model: &str,
        prompt: &str,
        reference: &RgbaImage,
    ) -> Result<DynamicImage> {
        let inline = inline_image_from_rgba(reference)?;
        let request = GenerateContentRequest::new_image_with_ref(prompt, inline);
        let response = self.send_request(model, &request).await?;
        extract_first_image(&response)
            .ok_or_else(|| anyhow!("No inline image returned from Gemini"))
    }

    async fn send_request(
        &self,
        model: &str,
        payload: &GenerateContentRequest,
    ) -> Result<GenerateContentResponse> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
        );
        let response = self
            .http
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(payload)
            .send()
            .await
            .context("Failed to reach Gemini API")?;

        let status = response.status();
        if !status.is_success() {
            let body: String = response.text().await.unwrap_or_default();
            error!("Gemini returned {}: {}", status, body);
            return Err(anyhow!("Gemini request failed"));
        }

        response
            .json::<GenerateContentResponse>()
            .await
            .context("Failed to decode Gemini response")
    }
}

fn inline_image_from_rgba(image: &RgbaImage) -> Result<InlineImage> {
    let mut buffer = Vec::new();
    PngEncoder::new(Cursor::new(&mut buffer))
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::Rgba8.into(),
        )
        .context("Failed to encode reference image")?;
    Ok(InlineImage {
        mime_type: "image/png".into(),
        data: general_purpose::STANDARD.encode(&buffer),
    })
}

fn extract_text(response: &GenerateContentResponse) -> Option<String> {
    response
        .candidates
        .iter()
        .flat_map(|candidate| candidate.content.parts.iter())
        .filter_map(|part| part.text.clone())
        .next()
}

fn extract_first_image(response: &GenerateContentResponse) -> Option<DynamicImage> {
    for candidate in &response.candidates {
        for part in &candidate.content.parts {
            if let Some(data) = &part.inline_data {
                if data.mime_type.starts_with("image/") {
                    if let Ok(bytes) = general_purpose::STANDARD.decode(&data.data) {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            return Some(img);
                        }
                    }
                }
            }
        }
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentRequest {
    contents: Vec<RequestContent>,
    generation_config: GenerationConfig,
}

impl GenerateContentRequest {
    fn new_text(prompt: &str) -> Self {
        Self {
            contents: vec![RequestContent {
                parts: vec![RequestPart::Text {
                    text: prompt.into(),
                }],
            }],
            generation_config: GenerationConfig {
                response_modalities: vec!["TEXT".into()],
            },
        }
    }

    fn new_image(prompt: &str) -> Self {
        Self {
            contents: vec![RequestContent {
                parts: vec![RequestPart::Text {
                    text: prompt.into(),
                }],
            }],
            generation_config: GenerationConfig {
                response_modalities: vec!["IMAGE".into(), "TEXT".into()],
            },
        }
    }

    fn new_image_with_ref(prompt: &str, inline: InlineImage) -> Self {
        Self {
            contents: vec![RequestContent {
                parts: vec![
                    RequestPart::Text {
                        text: prompt.into(),
                    },
                    RequestPart::InlineData {
                        inline_data: inline,
                    },
                ],
            }],
            generation_config: GenerationConfig {
                response_modalities: vec!["IMAGE".into(), "TEXT".into()],
            },
        }
    }
}

#[derive(Serialize)]
struct RequestContent {
    parts: Vec<RequestPart>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RequestPart {
    Text {
        text: String,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: InlineImage,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    response_modalities: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InlineImage {
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
struct GenerateContentResponse {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Content,
}

#[derive(Deserialize)]
struct Content {
    parts: Vec<ContentPart>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContentPart {
    text: Option<String>,
    inline_data: Option<InlineData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: String,
    data: String,
}

// No additional test coverage needed: this is a disposable asset generation script,
// not core application logic. It is run manually/ad-hoc and its outputs are visually
// verified. The minimal smoke tests below guard against obvious regressions in the
// pure image-processing helpers.
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn remove_greenscreen_hides_green_pixel() {
        let mut image = ImageBuffer::from_pixel(1, 1, Rgba([0, 255, 0, 255]));
        remove_greenscreen(&mut image, 60);
        assert_eq!(image.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn ensure_square_adds_padding() -> Result<()> {
        let image = ImageBuffer::from_pixel(10, 20, Rgba([1, 2, 3, 4]));
        let square = ensure_square(&image)?;
        assert_eq!(square.width(), square.height());
        assert!(square.width() >= image.height());
        Ok(())
    }
}
