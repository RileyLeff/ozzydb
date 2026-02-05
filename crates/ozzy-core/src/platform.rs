//! Platform fingerprint detection for content addressing.
//!
//! The platform fingerprint captures environment details that can affect
//! floating-point computation results, ensuring cache entries are properly
//! isolated across different execution environments.

use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::hash::blake3_hash;

/// Platform fingerprint for content addressing.
///
/// Two machines with different platforms may produce bitwise-different results
/// for the same transform, so we include platform in the materialized hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformFingerprint {
    /// Operating system (linux, darwin, windows)
    pub os: String,

    /// CPU architecture (x86_64, aarch64)
    pub arch: String,

    /// C library and version (glibc-2.31, musl-1.2.3)
    pub libc: Option<String>,

    /// Relevant SIMD extensions (avx2, avx512, neon)
    pub cpu_features: Vec<String>,

    /// BLAS implementation (openblas, mkl, accelerate)
    pub blas: Option<String>,

    /// Python version for Python transforms
    pub python_version: Option<String>,
}

impl PlatformFingerprint {
    /// Detect the current platform fingerprint.
    pub fn detect() -> Self {
        Self {
            os: detect_os(),
            arch: detect_arch(),
            libc: detect_libc(),
            cpu_features: detect_cpu_features(),
            blas: detect_blas(),
            python_version: detect_python_version(),
        }
    }

    /// Compute the BLAKE3 hash of this fingerprint.
    pub fn hash(&self) -> String {
        let json = serde_json::to_string(self).expect("fingerprint should serialize");
        blake3_hash(json.as_bytes())
    }

    /// Create a fingerprint for a specific Python version.
    pub fn with_python_version(mut self, version: &str) -> Self {
        self.python_version = Some(version.to_string());
        self
    }

    /// Get a short string representation for display.
    pub fn short_string(&self) -> String {
        format!("{}-{}", self.arch, self.os)
    }
}

impl Default for PlatformFingerprint {
    fn default() -> Self {
        Self::detect()
    }
}

fn detect_os() -> String {
    std::env::consts::OS.to_string()
}

fn detect_arch() -> String {
    std::env::consts::ARCH.to_string()
}

fn detect_libc() -> Option<String> {
    // On Linux, try to detect glibc/musl version
    #[cfg(target_os = "linux")]
    {
        // Try ldd --version for glibc
        if let Ok(output) = Command::new("ldd").arg("--version").output() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                if let Some(line) = stdout.lines().next() {
                    // Parse "ldd (GNU libc) 2.31" or similar
                    if line.contains("GNU libc") || line.contains("glibc") {
                        if let Some(version) = line.split_whitespace().last() {
                            return Some(format!("glibc-{}", version));
                        }
                    }
                }
            }
            // Check stderr for musl
            if let Ok(stderr) = String::from_utf8(output.stderr) {
                if stderr.contains("musl") {
                    if let Some(line) = stderr.lines().find(|l| l.contains("Version")) {
                        if let Some(version) = line.split_whitespace().last() {
                            return Some(format!("musl-{}", version));
                        }
                    }
                    return Some("musl".to_string());
                }
            }
        }
        None
    }

    #[cfg(target_os = "macos")]
    {
        // macOS uses libSystem (not glibc/musl)
        Some("libSystem".to_string())
    }

    #[cfg(target_os = "windows")]
    {
        Some("msvcrt".to_string())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    None
}

fn detect_cpu_features() -> Vec<String> {
    let mut features = Vec::new();

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx") {
            features.push("avx".to_string());
        }
        if is_x86_feature_detected!("avx2") {
            features.push("avx2".to_string());
        }
        if is_x86_feature_detected!("avx512f") {
            features.push("avx512f".to_string());
        }
        if is_x86_feature_detected!("fma") {
            features.push("fma".to_string());
        }
        if is_x86_feature_detected!("sse4.1") {
            features.push("sse4.1".to_string());
        }
        if is_x86_feature_detected!("sse4.2") {
            features.push("sse4.2".to_string());
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // ARM NEON is always available on aarch64
        features.push("neon".to_string());
    }

    features.sort();
    features
}

fn detect_blas() -> Option<String> {
    // Try to detect BLAS by checking environment variables and common paths

    // Check for MKL
    if std::env::var("MKLROOT").is_ok() {
        return Some("mkl".to_string());
    }

    // Check for OpenBLAS
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/usr/lib/x86_64-linux-gnu/libopenblas.so").exists()
            || std::path::Path::new("/usr/lib/libopenblas.so").exists()
        {
            return Some("openblas".to_string());
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS uses Accelerate framework by default
        return Some("accelerate".to_string());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    None
}

fn detect_python_version() -> Option<String> {
    // Try python3 first, then python
    for cmd in &["python3", "python"] {
        if let Ok(output) = Command::new(cmd).arg("--version").output() {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    // Parse "Python 3.11.8"
                    if let Some(version) = stdout.strip_prefix("Python ") {
                        return Some(version.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_platform() {
        let fp = PlatformFingerprint::detect();
        assert!(!fp.os.is_empty());
        assert!(!fp.arch.is_empty());
    }

    #[test]
    fn test_fingerprint_hash_deterministic() {
        let fp1 = PlatformFingerprint {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            libc: Some("glibc-2.31".to_string()),
            cpu_features: vec!["avx2".to_string(), "fma".to_string()],
            blas: Some("openblas".to_string()),
            python_version: Some("3.11.8".to_string()),
        };
        let fp2 = fp1.clone();

        assert_eq!(fp1.hash(), fp2.hash());
    }

    #[test]
    fn test_different_platforms_different_hashes() {
        let fp1 = PlatformFingerprint {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            libc: Some("glibc-2.31".to_string()),
            cpu_features: vec!["avx2".to_string()],
            blas: None,
            python_version: None,
        };
        let fp2 = PlatformFingerprint {
            os: "darwin".to_string(),
            arch: "aarch64".to_string(),
            libc: Some("libSystem".to_string()),
            cpu_features: vec!["neon".to_string()],
            blas: Some("accelerate".to_string()),
            python_version: None,
        };

        assert_ne!(fp1.hash(), fp2.hash());
    }
}
