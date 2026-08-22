//! AppPackage Primitives for SpaceKit Network
//!
//! This module provides the core data structures for the SpaceKit App Package system,
//! enabling signed, versioned application bundles that can contain WASM, HTML, CSS,
//! JavaScript, and any other files needed to run an application.
//!
//! AppPackages are the foundation of the SpaceKit AppStore, allowing developers to
//! publish applications that users can discover, verify, and run securely.

use crate::v1::crypto::quantum::SPHINCSSignature;
use crate::v1::fact::{AccessPolicy, FactID, LicenseType, Timestamp};
use crate::v1::identity::QuantumDID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for an app (SHA-256 hash of creator_did + app_name + initial_version)
pub type AppID = [u8; 32];

/// Semantic version components
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: Option<String>,
    pub build: Option<String>,
}

impl SemVer {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: None,
            build: None,
        }
    }

    pub fn parse(version: &str) -> Result<Self, String> {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() < 3 {
            return Err(format!("Invalid version format: {}", version));
        }

        let major = parts[0].parse().map_err(|_| "Invalid major version")?;
        let minor = parts[1].parse().map_err(|_| "Invalid minor version")?;

        // Handle patch with possible prerelease/build metadata
        let patch_str = parts[2];
        let (patch_num, prerelease, build) = if let Some(idx) = patch_str.find('-') {
            let (p, rest) = patch_str.split_at(idx);
            let rest = &rest[1..]; // Skip the '-'
            if let Some(build_idx) = rest.find('+') {
                let (pre, b) = rest.split_at(build_idx);
                (
                    p.parse().map_err(|_| "Invalid patch version")?,
                    Some(pre.to_string()),
                    Some(b[1..].to_string()),
                )
            } else {
                (
                    p.parse().map_err(|_| "Invalid patch version")?,
                    Some(rest.to_string()),
                    None,
                )
            }
        } else if let Some(idx) = patch_str.find('+') {
            let (p, b) = patch_str.split_at(idx);
            (
                p.parse().map_err(|_| "Invalid patch version")?,
                None,
                Some(b[1..].to_string()),
            )
        } else {
            (
                patch_str.parse().map_err(|_| "Invalid patch version")?,
                None,
                None,
            )
        };

        Ok(Self {
            major,
            minor,
            patch: patch_num,
            prerelease,
            build,
        })
    }

    pub fn to_string(&self) -> String {
        let mut s = format!("{}.{}.{}", self.major, self.minor, self.patch);
        if let Some(ref pre) = self.prerelease {
            s.push('-');
            s.push_str(pre);
        }
        if let Some(ref build) = self.build {
            s.push('+');
            s.push_str(build);
        }
        s
    }
}

impl Default for SemVer {
    fn default() -> Self {
        Self::new(1, 0, 0)
    }
}

/// Core AppPackage structure - a signed bundle of application files
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppPackage {
    /// Unique app identifier (deterministic from creator + name)
    pub app_id: AppID,

    /// Semantic version
    pub version: SemVer,

    /// Creation timestamp
    pub created_at: Timestamp,

    /// Creator identity
    pub creator_did: QuantumDID,

    /// SPHINCS+ signature over the manifest
    pub signature: SPHINCSSignature,

    /// Package manifest describing contents
    pub manifest: AppManifest,

    /// References to content stored as FactPackages
    pub content_refs: Vec<ContentRef>,

    /// License type
    pub license_type: LicenseType,

    /// Access policy for the app
    pub access_policy: AccessPolicy,

    /// Dependencies on other AppPackages
    pub dependencies: Vec<AppDependency>,

    /// App category for discovery
    pub category: AppCategory,

    /// Pricing information
    pub pricing: AppPricing,
}

/// App manifest describing the package contents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppManifest {
    /// Human-readable app name
    pub name: String,

    /// App description
    pub description: String,

    /// Short tagline for listings
    pub tagline: Option<String>,

    /// Entry points for the app
    pub entry_points: Vec<EntryPoint>,

    /// Required permissions
    pub permissions: Vec<Permission>,

    /// Content types included in the package
    pub content_types: Vec<ContentType>,

    /// Total size of all content in bytes
    pub total_size: u64,

    /// SHA-256 hash of all content (for integrity verification)
    pub checksum: [u8; 32],

    /// Icon reference (path within package or external URL)
    pub icon: Option<String>,

    /// Screenshots/preview images
    pub screenshots: Vec<String>,

    /// Keywords for search
    pub keywords: Vec<String>,

    /// Minimum SpaceKit runtime version required
    pub min_runtime_version: Option<SemVer>,

    /// Supported platforms
    pub platforms: Vec<Platform>,
}

/// Reference to content stored as a FactPackage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentRef {
    /// Path within the app package (e.g., "main.wasm", "index.html")
    pub path: String,

    /// Content type
    pub content_type: ContentType,

    /// Size in bytes
    pub size: u64,

    /// SHA-256 hash of the content
    pub hash: [u8; 32],

    /// Compression algorithm used
    pub compression: CompressionAlgorithm,

    /// Whether the content is encrypted
    pub encrypted: bool,

    /// Reference to the FactPackage storing this content
    pub fact_id: FactID,
}

/// Entry points for launching the app
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EntryPoint {
    /// WebAssembly module
    Wasm {
        path: String,
        exports: Vec<String>,
        /// Memory requirements in pages (64KB each)
        memory_pages: Option<u32>,
    },

    /// HTML page
    Html {
        path: String,
        /// Whether this is the main entry point
        is_main: bool,
    },

    /// JavaScript module
    Script {
        path: String,
        module_type: ScriptModuleType,
    },

    /// React/TSX component
    Component {
        path: String,
        component_name: String,
        props_schema: Option<String>,
    },

    /// API endpoint definition
    Api { path: String, routes: Vec<ApiRoute> },
}

/// Script module types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScriptModuleType {
    ESModule,
    CommonJS,
    UMD,
    IIFE,
}

/// API route definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiRoute {
    pub method: String,
    pub path: String,
    pub handler: String,
}

/// Permissions that an app may request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Permission {
    /// Access to local storage
    Storage { max_bytes: Option<u64> },

    /// Network access
    Network { allowed_hosts: Vec<String> },

    /// File system access
    FileSystem { paths: Vec<String>, write: bool },

    /// Camera access
    Camera,

    /// Microphone access
    Microphone,

    /// Geolocation access
    Geolocation,

    /// Access to user's DID/identity
    Identity { read_only: bool },

    /// Access to wallet/payments
    Wallet { max_amount: Option<u64> },

    /// Access to other apps via IPC
    InterApp { app_ids: Vec<AppID> },

    /// Access to clipboard
    Clipboard { write: bool },

    /// Push notifications
    Notifications,

    /// Background execution
    Background,

    /// Custom permission
    Custom { name: String, description: String },
}

/// Content types that can be included in an app package
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ContentType {
    /// WebAssembly binary
    Wasm,

    /// HTML document
    Html,

    /// CSS stylesheet
    Css,

    /// JavaScript
    JavaScript,

    /// TypeScript (source)
    TypeScript,

    /// JSX/TSX React components
    React,

    /// JSON data/config
    Json,

    /// Image (PNG, JPG, SVG, WebP)
    Image { format: String },

    /// Font file
    Font { format: String },

    /// Audio file
    Audio { format: String },

    /// Video file
    Video { format: String },

    /// Markdown documentation
    Markdown,

    /// Binary data
    Binary { mime_type: String },

    /// Other/unknown
    Other { mime_type: String },
}

impl ContentType {
    /// Detect content type from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "wasm" => Self::Wasm,
            "html" | "htm" => Self::Html,
            "css" => Self::Css,
            "js" | "mjs" => Self::JavaScript,
            "ts" | "mts" => Self::TypeScript,
            "jsx" | "tsx" => Self::React,
            "json" => Self::Json,
            "png" => Self::Image {
                format: "png".to_string(),
            },
            "jpg" | "jpeg" => Self::Image {
                format: "jpeg".to_string(),
            },
            "svg" => Self::Image {
                format: "svg".to_string(),
            },
            "webp" => Self::Image {
                format: "webp".to_string(),
            },
            "gif" => Self::Image {
                format: "gif".to_string(),
            },
            "ico" => Self::Image {
                format: "ico".to_string(),
            },
            "woff" | "woff2" => Self::Font {
                format: ext.to_string(),
            },
            "ttf" | "otf" => Self::Font {
                format: ext.to_string(),
            },
            "mp3" | "ogg" | "wav" | "flac" => Self::Audio {
                format: ext.to_string(),
            },
            "mp4" | "webm" | "avi" => Self::Video {
                format: ext.to_string(),
            },
            "md" | "markdown" => Self::Markdown,
            _ => Self::Other {
                mime_type: format!("application/{}", ext),
            },
        }
    }

    /// Get MIME type string
    pub fn mime_type(&self) -> String {
        match self {
            Self::Wasm => "application/wasm".to_string(),
            Self::Html => "text/html".to_string(),
            Self::Css => "text/css".to_string(),
            Self::JavaScript => "application/javascript".to_string(),
            Self::TypeScript => "application/typescript".to_string(),
            Self::React => "text/jsx".to_string(),
            Self::Json => "application/json".to_string(),
            Self::Image { format } => format!("image/{}", format),
            Self::Font { format } => format!("font/{}", format),
            Self::Audio { format } => format!("audio/{}", format),
            Self::Video { format } => format!("video/{}", format),
            Self::Markdown => "text/markdown".to_string(),
            Self::Binary { mime_type } | Self::Other { mime_type } => mime_type.clone(),
        }
    }
}

/// Compression algorithms for content
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompressionAlgorithm {
    None,
    Gzip,
    Zstd,
    Lz4,
    Brotli,
}

impl Default for CompressionAlgorithm {
    fn default() -> Self {
        Self::Zstd
    }
}

/// Supported platforms
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Platform {
    Web,
    Desktop { os: Vec<String> },
    Mobile { os: Vec<String> },
    Server,
    Embedded,
    Any,
}

impl Default for Platform {
    fn default() -> Self {
        Self::Any
    }
}

/// Dependency on another AppPackage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppDependency {
    /// App ID of the dependency
    pub app_id: AppID,

    /// Version constraint (e.g., ">=1.0.0", "^2.0.0")
    pub version_constraint: String,

    /// Whether this dependency is optional
    pub optional: bool,

    /// Human-readable name for error messages
    pub name: Option<String>,
}

/// App categories for the marketplace
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AppCategory {
    /// Productivity tools
    Productivity,

    /// Social and communication
    Social,

    /// Finance and payments
    Finance,

    /// Games
    Games,

    /// Entertainment (music, video, etc.)
    Entertainment,

    /// Developer tools
    Developer,

    /// Education
    Education,

    /// Health and fitness
    Health,

    /// News and media
    News,

    /// Utilities
    Utilities,

    /// AI and machine learning
    AI,

    /// Storage and files
    Storage,

    /// Identity and security
    Security,

    /// Lifestyle
    Lifestyle,

    /// Business
    Business,

    /// Custom category
    Custom(String),
}

impl Default for AppCategory {
    fn default() -> Self {
        Self::Utilities
    }
}

/// Pricing information for the app
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AppPricing {
    /// Free app
    Free,

    /// One-time purchase
    Paid {
        /// Price in smallest token units (e.g., lamports, wei)
        amount: u64,
        /// Token symbol (e.g., "SWTCHX", "SOL")
        token: String,
    },

    /// Subscription-based
    Subscription {
        amount: u64,
        token: String,
        /// Billing period in seconds
        period: u64,
    },

    /// Pay what you want
    PayWhatYouWant {
        minimum: Option<u64>,
        suggested: Option<u64>,
        token: String,
    },

    /// Free with in-app purchases
    Freemium,
}

impl Default for AppPricing {
    fn default() -> Self {
        Self::Free
    }
}

/// Result of loading an AppPackage
#[derive(Debug, Clone)]
pub struct LoadedApp {
    pub app_id: AppID,
    pub manifest: AppManifest,
    pub files: HashMap<String, Vec<u8>>,
    pub verified: bool,
    pub creator_did: QuantumDID,
}

/// App verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppVerificationResult {
    pub signature_valid: bool,
    pub creator_verified: bool,
    pub content_integrity: bool,
    pub all_dependencies_available: bool,
    pub permissions_acceptable: bool,
    pub overall_valid: bool,
    pub warnings: Vec<String>,
}

impl AppPackage {
    /// Compute the app ID from creator DID and app name
    pub fn compute_app_id(creator_did: &QuantumDID, name: &str) -> AppID {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(creator_did.to_bytes());
        hasher.update(name.as_bytes());
        hasher.finalize().into()
    }

    /// Compute manifest hash for signing
    pub fn compute_manifest_hash(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(
            serde_json::to_string(&self.manifest)
                .unwrap_or_default()
                .as_bytes(),
        );
        hasher.update(&self.app_id);
        hasher.update(self.version.to_string().as_bytes());
        hasher.finalize().into()
    }

    /// Check if all content hashes are valid
    pub fn verify_content_integrity(&self, content_map: &HashMap<String, Vec<u8>>) -> bool {
        use sha2::{Digest, Sha256};

        for content_ref in &self.content_refs {
            match content_map.get(&content_ref.path) {
                Some(data) => {
                    let hash: [u8; 32] = Sha256::digest(data).into();
                    if hash != content_ref.hash {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// Get the main entry point
    pub fn main_entry_point(&self) -> Option<&EntryPoint> {
        // Prefer WASM, then HTML, then Component
        self.manifest
            .entry_points
            .iter()
            .find(|e| matches!(e, EntryPoint::Wasm { .. }))
            .or_else(|| {
                self.manifest
                    .entry_points
                    .iter()
                    .find(|e| matches!(e, EntryPoint::Html { is_main: true, .. }))
            })
            .or_else(|| {
                self.manifest
                    .entry_points
                    .iter()
                    .find(|e| matches!(e, EntryPoint::Component { .. }))
            })
            .or_else(|| self.manifest.entry_points.first())
    }

    /// Check if the app requires payment
    pub fn requires_payment(&self) -> bool {
        !matches!(self.pricing, AppPricing::Free | AppPricing::Freemium)
    }

    /// Get total storage size
    pub fn total_size(&self) -> u64 {
        self.content_refs.iter().map(|r| r.size).sum()
    }
}

impl Default for AppManifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            tagline: None,
            entry_points: Vec::new(),
            permissions: Vec::new(),
            content_types: Vec::new(),
            total_size: 0,
            checksum: [0u8; 32],
            icon: None,
            screenshots: Vec::new(),
            keywords: Vec::new(),
            min_runtime_version: None,
            platforms: vec![Platform::Any],
        }
    }
}
