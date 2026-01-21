// src/icons.rs
use log::{info, warn};
use std::io;
use std::path::{Path, PathBuf};

/// Represents the source of an icon - either a local file path or a service name
#[derive(Debug)]
pub enum IconSource {
    FilePath(PathBuf),
    ServiceName(String),
}

/// Represents the desired icon format
#[derive(Debug, Clone, Copy)]
pub enum IconFormat {
    Svg,
    Png,
    Ico,
}

/// Represents which CDN source was used to download an icon
#[derive(Debug, Clone, Copy)]
pub enum IconSourceType {
    DashboardIcons,
    SimpleIcons,
}

impl IconSourceType {
    fn cache_subdir(&self) -> &str {
        match self {
            IconSourceType::DashboardIcons => "dashboard-icons",
            IconSourceType::SimpleIcons => "simple-icons",
        }
    }
}

/// Main icon resolver that handles downloading, caching, and format conversion
pub struct IconResolver {
    cache_dir: PathBuf,
    dashboard_icons_url: String,
    simple_icons_url: String,
}

impl IconResolver {
    /// Creates a new IconResolver with cache directory at ~/.cache/clovis/icons/
    pub fn new() -> io::Result<Self> {
        let cache_dir = if let Some(cache_home) = dirs::cache_dir() {
            cache_home.join("clovis").join("icons")
        } else {
            // Fallback to home directory if cache_dir is not available
            let home = dirs::home_dir().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Could not find home directory")
            })?;
            home.join(".cache").join("clovis").join("icons")
        };

        // Create cache directory structure
        std::fs::create_dir_all(&cache_dir)?;
        std::fs::create_dir_all(cache_dir.join("dashboard-icons").join("svg"))?;
        std::fs::create_dir_all(cache_dir.join("dashboard-icons").join("png"))?;
        std::fs::create_dir_all(cache_dir.join("dashboard-icons").join("ico"))?;
        std::fs::create_dir_all(cache_dir.join("simple-icons").join("svg"))?;
        std::fs::create_dir_all(cache_dir.join("simple-icons").join("png"))?;
        std::fs::create_dir_all(cache_dir.join("simple-icons").join("ico"))?;

        Ok(Self {
            cache_dir,
            dashboard_icons_url: "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg"
                .to_string(),
            simple_icons_url: "https://cdn.jsdelivr.net/npm/simple-icons@latest/icons"
                .to_string(),
        })
    }

    /// Resolves an input string to determine if it's a file path or service name
    pub fn resolve_icon(&self, input: &str) -> IconSource {
        // Check if it looks like a file path (contains path separators or file extensions)
        if input.contains('/') || input.contains('\\') || input.contains('.') {
            let path = PathBuf::from(input);
            IconSource::FilePath(path)
        } else {
            // It's a service name - normalize to kebab-case
            let normalized = self.normalize_service_name(input);
            IconSource::ServiceName(normalized)
        }
    }

    /// Normalizes a service name to kebab-case
    fn normalize_service_name(&self, name: &str) -> String {
        name.to_lowercase()
            .replace(' ', "-")
            .replace('_', "-")
    }

    /// Main entry point: gets an icon path in the desired format
    pub fn get_icon_path(&self, source: IconSource, format: IconFormat) -> io::Result<PathBuf> {
        match source {
            IconSource::FilePath(path) => {
                // Validate that the file exists
                if !path.exists() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("Icon file not found: {}", path.display()),
                    ));
                }

                // If the file is already in the correct format, return it
                if self.is_correct_format(&path, format) {
                    Ok(path)
                } else {
                    // Need to convert the format
                    self.convert_local_icon(&path, format)
                }
            }
            IconSource::ServiceName(service_name) => {
                // Check if we have it cached in the desired format
                if let Some(cached_path) = self.find_cached_icon(&service_name, format) {
                    info!(
                        "Using cached icon for '{}' in format {:?}",
                        service_name, format
                    );
                    return Ok(cached_path);
                }

                // Download the icon
                let (svg_data, source_type) = self.download_icon(&service_name)?;

                // Cache the SVG
                let svg_path = self.cache_icon(&service_name, source_type, &svg_data)?;

                // Convert to the desired format if not SVG
                match format {
                    IconFormat::Svg => Ok(svg_path),
                    IconFormat::Png => {
                        let png_data = self.convert_svg_to_png(&svg_data)?;
                        let png_path = self.cache_dir
                            .join(source_type.cache_subdir())
                            .join("png")
                            .join(format!("{}.png", service_name));
                        std::fs::write(&png_path, png_data)?;
                        Ok(png_path)
                    }
                    IconFormat::Ico => {
                        let ico_data = self.convert_svg_to_ico(&svg_data)?;
                        let ico_path = self.cache_dir
                            .join(source_type.cache_subdir())
                            .join("ico")
                            .join(format!("{}.ico", service_name));
                        std::fs::write(&ico_path, ico_data)?;
                        Ok(ico_path)
                    }
                }
            }
        }
    }

    /// Checks if a file is in the correct format
    fn is_correct_format(&self, path: &Path, format: IconFormat) -> bool {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match format {
            IconFormat::Svg => extension == "svg",
            IconFormat::Png => extension == "png",
            IconFormat::Ico => extension == "ico",
        }
    }

    /// Converts a local icon file to the desired format
    fn convert_local_icon(&self, path: &Path, format: IconFormat) -> io::Result<PathBuf> {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        // Read the source file
        let data = std::fs::read(path)?;

        // For SVG sources, convert using our SVG converters
        if extension == "svg" {
            let converted_data = match format {
                IconFormat::Svg => return Ok(path.to_path_buf()),
                IconFormat::Png => self.convert_svg_to_png(&data)?,
                IconFormat::Ico => self.convert_svg_to_ico(&data)?,
            };

            // Save to a temporary location
            let temp_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("icon");
            let temp_path = self.cache_dir.join(format!(
                "temp_{}.{}",
                temp_name,
                match format {
                    IconFormat::Svg => "svg",
                    IconFormat::Png => "png",
                    IconFormat::Ico => "ico",
                }
            ));
            std::fs::write(&temp_path, converted_data)?;
            Ok(temp_path)
        } else {
            // For other formats (png, jpg, etc.), use image library to convert
            match format {
                IconFormat::Svg => Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Cannot convert raster image to SVG",
                )),
                IconFormat::Png => {
                    let img = image::load_from_memory(&data).map_err(|e| {
                        io::Error::new(io::ErrorKind::InvalidData, format!("Image error: {}", e))
                    })?;
                    let temp_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("icon");
                    let temp_path = self.cache_dir.join(format!("temp_{}.png", temp_name));
                    img.save(&temp_path).map_err(|e| {
                        io::Error::new(io::ErrorKind::Other, format!("Failed to save PNG: {}", e))
                    })?;
                    Ok(temp_path)
                }
                IconFormat::Ico => {
                    let img = image::load_from_memory(&data).map_err(|e| {
                        io::Error::new(io::ErrorKind::InvalidData, format!("Image error: {}", e))
                    })?;
                    let rgba = img.to_rgba8();
                    let (width, height) = rgba.dimensions();

                    let icon_image = ico::IconImage::from_rgba_data(width, height, rgba.into_raw());
                    let icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
                    let mut icon_dir = icon_dir;
                    icon_dir.add_entry(ico::IconDirEntry::encode(&icon_image).map_err(|e| {
                        io::Error::new(io::ErrorKind::Other, format!("Failed to encode ICO: {}", e))
                    })?);

                    let temp_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("icon");
                    let temp_path = self.cache_dir.join(format!("temp_{}.ico", temp_name));
                    let mut file = std::fs::File::create(&temp_path)?;
                    icon_dir.write(&mut file).map_err(|e| {
                        io::Error::new(io::ErrorKind::Other, format!("Failed to write ICO: {}", e))
                    })?;
                    Ok(temp_path)
                }
            }
        }
    }

    /// Checks if an icon is cached in the desired format
    fn find_cached_icon(&self, service_name: &str, format: IconFormat) -> Option<PathBuf> {
        let format_dir = match format {
            IconFormat::Svg => "svg",
            IconFormat::Png => "png",
            IconFormat::Ico => "ico",
        };

        let extension = match format {
            IconFormat::Svg => "svg",
            IconFormat::Png => "png",
            IconFormat::Ico => "ico",
        };

        // Check both Dashboard Icons and Simple Icons caches
        for source_type in [IconSourceType::DashboardIcons, IconSourceType::SimpleIcons] {
            let path = self.cache_dir
                .join(source_type.cache_subdir())
                .join(format_dir)
                .join(format!("{}.{}", service_name, extension));

            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    /// Downloads an icon from the CDN with fallback from Dashboard Icons to Simple Icons
    fn download_icon(&self, service_name: &str) -> io::Result<(Vec<u8>, IconSourceType)> {
        // Try Dashboard Icons first
        let dashboard_url = format!("{}/{}.svg", self.dashboard_icons_url, service_name);
        info!("Attempting to download icon from Dashboard Icons: {}", dashboard_url);

        match self.fetch_url(&dashboard_url) {
            Ok(data) => {
                info!("Successfully downloaded icon '{}' from Dashboard Icons", service_name);
                return Ok((data, IconSourceType::DashboardIcons));
            }
            Err(e) => {
                warn!("Failed to download from Dashboard Icons: {}. Trying Simple Icons...", e);
            }
        }

        // Fallback to Simple Icons
        let simple_url = format!("{}/{}.svg", self.simple_icons_url, service_name);
        info!("Attempting to download icon from Simple Icons: {}", simple_url);

        match self.fetch_url(&simple_url) {
            Ok(data) => {
                info!("Successfully downloaded icon '{}' from Simple Icons", service_name);
                Ok((data, IconSourceType::SimpleIcons))
            }
            Err(e) => {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "Icon '{}' not found in Dashboard Icons or Simple Icons: {}",
                        service_name, e
                    ),
                ))
            }
        }
    }

    /// Fetches content from a URL using reqwest blocking client
    fn fetch_url(&self, url: &str) -> io::Result<Vec<u8>> {
        let response = reqwest::blocking::get(url).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("HTTP request failed: {}", e),
            )
        })?;

        if !response.status().is_success() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("HTTP {} for URL: {}", response.status(), url),
            ));
        }

        let bytes = response.bytes().map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to read response: {}", e),
            )
        })?;

        Ok(bytes.to_vec())
    }

    /// Caches an icon (SVG format) to disk
    fn cache_icon(
        &self,
        service_name: &str,
        source_type: IconSourceType,
        data: &[u8],
    ) -> io::Result<PathBuf> {
        let svg_path = self.cache_dir
            .join(source_type.cache_subdir())
            .join("svg")
            .join(format!("{}.svg", service_name));

        std::fs::write(&svg_path, data)?;
        info!(
            "Cached icon '{}' from {} at {}",
            service_name,
            source_type.cache_subdir(),
            svg_path.display()
        );
        Ok(svg_path)
    }

    /// Converts SVG data to PNG format using resvg
    fn convert_svg_to_png(&self, svg_data: &[u8]) -> io::Result<Vec<u8>> {
        let opts = usvg::Options::default();
        let tree = usvg::Tree::from_data(svg_data, &opts).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse SVG: {}", e),
            )
        })?;

        let size = tree.size();
        let target_size = 512; // 512x512 PNG
        let scale = (target_size as f32 / size.width().max(size.height())).min(1.0);

        let width = (size.width() * scale) as u32;
        let height = (size.height() * scale) as u32;

        let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Failed to create pixmap")
        })?;

        let transform = tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let png_data = pixmap.encode_png().map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("Failed to encode PNG: {}", e))
        })?;

        Ok(png_data)
    }

    /// Converts SVG data to ICO format (multi-resolution)
    fn convert_svg_to_ico(&self, svg_data: &[u8]) -> io::Result<Vec<u8>> {
        let opts = usvg::Options::default();
        let tree = usvg::Tree::from_data(svg_data, &opts).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse SVG: {}", e),
            )
        })?;

        let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);

        // Generate multiple resolutions for ICO (16x16, 32x32, 48x48, 256x256)
        for size in [16, 32, 48, 256] {
            let tree_size = tree.size();
            let scale = size as f32 / tree_size.width().max(tree_size.height());

            let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::Other, "Failed to create pixmap")
            })?;

            let transform = tiny_skia::Transform::from_scale(scale, scale);
            resvg::render(&tree, transform, &mut pixmap.as_mut());

            let rgba_data = pixmap.data().to_vec();
            let icon_image = ico::IconImage::from_rgba_data(size, size, rgba_data);
            let entry = ico::IconDirEntry::encode(&icon_image).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("Failed to encode ICO entry: {}", e))
            })?;
            icon_dir.add_entry(entry);
        }

        let mut ico_data = Vec::new();
        icon_dir.write(&mut ico_data).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("Failed to write ICO: {}", e))
        })?;

        Ok(ico_data)
    }
}
