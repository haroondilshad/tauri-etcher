/*
 * Flash module - handles writing images to drives
 * 
 * Replaces the Node.js child-writer.ts and etcher-sdk multi-write functionality.
 */

use crate::drives::unmount_disk;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

// Decompression imports
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use xz2::read::XzDecoder;

/// Flash progress state sent to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashProgress {
    pub bytes_written: u64,
    pub total_bytes: u64,
    pub percentage: f64,
    pub speed: f64,           // bytes per second
    pub eta: Option<f64>,     // seconds remaining
    pub stage: String,        // "flashing" or "verifying"
    pub active: u32,          // number of active destinations
    pub failed: u32,          // number of failed destinations
}

/// Flash result returned when complete
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashResult {
    pub bytes_written: u64,
    pub devices: DeviceResult,
    pub errors: Vec<String>,
    pub source_checksum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceResult {
    pub successful: u32,
    pub failed: u32,
}

/// Compression type detection
#[derive(Debug, Clone, PartialEq)]
enum Compression {
    None,
    Gzip,
    Xz,
    Bz2,
    Zip,
}

/// Detect compression type from file extension
fn detect_compression(path: &str) -> Compression {
    let lower = path.to_lowercase();
    if lower.ends_with(".gz") || lower.ends_with(".gzip") {
        Compression::Gzip
    } else if lower.ends_with(".xz") || lower.ends_with(".lzma") {
        Compression::Xz
    } else if lower.ends_with(".bz2") || lower.ends_with(".bzip2") {
        Compression::Bz2
    } else if lower.ends_with(".zip") {
        Compression::Zip
    } else {
        Compression::None
    }
}

/// Open a source file, automatically decompressing if needed
fn open_source(path: &str) -> Result<(Box<dyn Read + Send>, Option<u64>), String> {
    let file = File::open(path).map_err(|e| format!("Failed to open source: {}", e))?;
    let file_size = file.metadata().map(|m| m.len()).ok();
    
    match detect_compression(path) {
        Compression::Gzip => {
            let decoder = GzDecoder::new(BufReader::new(file));
            // For compressed files, we don't know the decompressed size
            Ok((Box::new(decoder), None))
        }
        Compression::Xz => {
            let decoder = XzDecoder::new(BufReader::new(file));
            Ok((Box::new(decoder), None))
        }
        Compression::Bz2 => {
            let decoder = BzDecoder::new(BufReader::new(file));
            Ok((Box::new(decoder), None))
        }
        Compression::Zip => {
            // For ZIP, we extract the first file
            let reader = BufReader::new(file);
            let zip_reader = open_zip_first_file(reader)?;
            Ok((zip_reader, None))
        }
        Compression::None => {
            Ok((Box::new(BufReader::new(file)), file_size))
        }
    }
}

/// Open the first file from a ZIP archive
fn open_zip_first_file(reader: BufReader<File>) -> Result<Box<dyn Read + Send>, String> {
    use std::io::Seek;
    
    // We need to use a different approach since zip crate requires Seek
    // Read the entire file into memory for small files, or use a temp file
    let mut file = reader.into_inner();
    file.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| format!("Failed to seek: {}", e))?;
    
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to open ZIP: {}", e))?;
    
    if archive.is_empty() {
        return Err("ZIP archive is empty".to_string());
    }
    
    // Find the first image-like file
    let mut target_index = 0;
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name().to_lowercase();
            if name.ends_with(".img") || name.ends_with(".iso") || name.ends_with(".raw") {
                target_index = i;
                break;
            }
        }
    }
    
    // Read the file into memory (ZIP requires random access)
    let mut zip_file = archive.by_index(target_index)
        .map_err(|e| format!("Failed to extract from ZIP: {}", e))?;
    
    let mut buffer = Vec::new();
    zip_file.read_to_end(&mut buffer)
        .map_err(|e| format!("Failed to read ZIP contents: {}", e))?;
    
    Ok(Box::new(std::io::Cursor::new(buffer)))
}

/// Global cancel flag
static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

/// Cancel the current flash operation
#[tauri::command]
pub fn cancel_flash() -> Result<(), String> {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
    Ok(())
}

/// Flash an image to a destination drive
/// 
/// This is the main function that replaces the Node.js sidecar functionality.
#[tauri::command]
pub async fn flash_image(
    source_path: String,
    dest_device: String,
    verify: bool,
    app: AppHandle,
) -> Result<FlashResult, String> {
    // Reset cancel flag
    CANCEL_FLAG.store(false, Ordering::SeqCst);
    
    // Unmount the destination disk first
    unmount_disk(dest_device.clone())?;
    
    // On macOS, use the raw device (rdisk) for faster writes
    let dest_path = if cfg!(target_os = "macos") && dest_device.contains("/dev/disk") {
        dest_device.replace("/dev/disk", "/dev/rdisk")
    } else {
        dest_device.clone()
    };
    
    // First try to open directly (works if running as root or with Full Disk Access)
    let direct_access = OpenOptions::new()
        .write(true)
        .open(&dest_path);
    
    match direct_access {
        Ok(dest) => {
            // We have direct access, use the fast path
            return flash_with_direct_access(source_path, dest, dest_path, verify, app).await;
        }
        Err(_e) => {
            // Direct access failed, use privileged helper
            return flash_with_helper(source_path, dest_path, verify, app).await;
        }
    }
}

/// Flash using direct file access (when running with privileges)
async fn flash_with_direct_access(
    source_path: String,
    mut dest: std::fs::File,
    dest_path: String,
    verify: bool,
    app: AppHandle,
) -> Result<FlashResult, String> {
    // Open source with automatic decompression
    let (mut source, known_size) = open_source(&source_path)?;
    
    // Buffer for reading/writing - 1MB for good performance
    const BUFFER_SIZE: usize = 1024 * 1024;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    
    // Tracking variables
    let mut bytes_written: u64 = 0;
    let mut hasher = crc32fast::Hasher::new();
    let start_time = std::time::Instant::now();
    let mut last_progress_time = start_time;
    let mut last_bytes = 0u64;
    
    // Flash loop
    loop {
        // Check for cancellation
        if CANCEL_FLAG.load(Ordering::SeqCst) {
            return Err("Flash cancelled by user".to_string());
        }
        
        // Read from source
        let bytes_read = source.read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;
        
        if bytes_read == 0 {
            break; // End of source
        }
        
        // Write to destination
        dest.write_all(&buffer[..bytes_read])
            .map_err(|e| format!("Write error: {}", e))?;
        
        // Update checksum
        hasher.update(&buffer[..bytes_read]);
        
        bytes_written += bytes_read as u64;
        
        // Emit progress every 100ms or so
        let now = std::time::Instant::now();
        if now.duration_since(last_progress_time).as_millis() >= 100 {
            let recent_elapsed = now.duration_since(last_progress_time).as_secs_f64();
            let recent_bytes = bytes_written - last_bytes;
            
            let speed = if recent_elapsed > 0.0 {
                recent_bytes as f64 / recent_elapsed
            } else {
                0.0
            };
            
            let (percentage, eta) = if let Some(total) = known_size {
                let pct = (bytes_written as f64 / total as f64) * 100.0;
                let remaining = total - bytes_written;
                let eta_secs = if speed > 0.0 {
                    Some(remaining as f64 / speed)
                } else {
                    None
                };
                (pct, eta_secs)
            } else {
                // Unknown size, just show bytes
                (0.0, None)
            };
            
            let progress = FlashProgress {
                bytes_written,
                total_bytes: known_size.unwrap_or(0),
                percentage,
                speed,
                eta,
                stage: "flashing".to_string(),
                active: 1,
                failed: 0,
            };
            
            let _ = app.emit("flash-progress", &progress);
            
            last_progress_time = now;
            last_bytes = bytes_written;
        }
    }
    
    // Sync to ensure all data is written
    // Note: sync_all() may fail on raw block devices (macOS returns ENOTTY)
    // This is safe to ignore as the data has been written
    if let Err(e) = dest.sync_all() {
        println!("Note: sync_all failed (normal for raw devices): {}", e);
    }
    
    let source_checksum = hasher.finalize();
    
    // Verification phase
    if verify {
        verify_write(&dest_path, bytes_written, source_checksum, &app)?;
    }
    
    Ok(FlashResult {
        bytes_written,
        devices: DeviceResult {
            successful: 1,
            failed: 0,
        },
        errors: Vec::new(),
        source_checksum,
    })
}

/// Flash using a privileged helper binary
/// This spawns the etcher-helper with sudo and communicates via socket
async fn flash_with_helper(
    source_path: String,
    dest_path: String,
    verify: bool,
    app: AppHandle,
) -> Result<FlashResult, String> {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::process::{Command, Stdio};
    use std::thread;
    
    // Find the helper binary
    let helper_path = find_helper_binary()?;
    
    // Start a TCP listener for the helper to connect to
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind socket: {}", e))?;
    let port = listener.local_addr()
        .map_err(|e| format!("Failed to get socket address: {}", e))?
        .port();
    
    
    // Build verify argument
    let verify_args: Vec<&str> = if verify { vec!["--verify"] } else { vec![] };
    
    // Create a temporary askpass script for macOS
    #[cfg(target_os = "macos")]
    let askpass_script = {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        
        let temp_dir = std::env::temp_dir();
        let askpass_path = temp_dir.join("etcher-askpass.sh");
        
        // Create the askpass script
        let script_content = r#"#!/bin/bash
osascript -e 'display dialog "balenaEtcher needs privileged access in order to flash disks.\n\nType your password to allow this." default answer "" with hidden answer buttons {"Cancel", "Ok"} default button "Ok" with icon caution' -e 'text returned of result' 2>/dev/null
"#;
        fs::write(&askpass_path, script_content)
            .map_err(|e| format!("Failed to create askpass script: {}", e))?;
        
        // Make it executable
        fs::set_permissions(&askpass_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set askpass permissions: {}", e))?;
        
        askpass_path
    };
    
    // Spawn the helper with sudo
    #[cfg(target_os = "macos")]
    let mut child = {
        // Run helper directly with sudo -A (no sh -c needed)
        let mut cmd = Command::new("sudo");
        cmd.env("SUDO_ASKPASS", &askpass_script)
            .arg("-A")
            .arg(&helper_path)
            .arg(&source_path)
            .arg(&dest_path)
            .arg(port.to_string());
        cmd.args(&verify_args);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn helper: {}", e))?
    };
    
    // On Linux, use pkexec or sudo
    #[cfg(target_os = "linux")]
    let mut child = {
        let mut cmd = Command::new("pkexec");
        cmd.arg(&helper_path)
            .arg(&source_path)
            .arg(&dest_path)
            .arg(port.to_string());
        cmd.args(&verify_args);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .or_else(|_| {
                // Fallback to sudo (may not show GUI prompt)
                let mut cmd = Command::new("sudo");
                cmd.arg(&helper_path)
                    .arg(&source_path)
                    .arg(&dest_path)
                    .arg(port.to_string());
                cmd.args(&verify_args);
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            })
            .map_err(|e| format!("Failed to spawn helper: {}", e))?
    };
    
    // On Windows, run helper directly (Windows handles UAC separately)
    #[cfg(target_os = "windows")]
    let mut child = {
        let mut cmd = Command::new(&helper_path);
        cmd.arg(&source_path)
            .arg(&dest_path)
            .arg(port.to_string());
        cmd.args(&verify_args);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn helper: {}", e))?
    };
    
    // Set a timeout for the helper to connect
    listener.set_nonblocking(true)
        .map_err(|e| format!("Failed to set non-blocking: {}", e))?;
    
    
    // Wait for connection with timeout
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(30);
    let stream = loop {
        // Check if child process has exited
        match child.try_wait() {
            Ok(Some(status)) => {
                // Child exited - read stderr for error details
                let mut stderr_output = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    use std::io::Read;
                    let _ = stderr.read_to_string(&mut stderr_output);
                }
                return Err(format!(
                    "Helper process exited with status {}. {}",
                    status,
                    if stderr_output.is_empty() { 
                        "User may have cancelled password prompt.".to_string() 
                    } else { 
                        format!("Error: {}", stderr_output.trim())
                    }
                ));
            }
            Ok(None) => {} // Still running
            Err(e) => {
                return Err(format!("Failed to check child status: {}", e));
            }
        }
        
        match listener.accept() {
            Ok((stream, _)) => {
                // Set stream back to blocking mode (it inherits non-blocking from listener)
                stream.set_nonblocking(false)
                    .map_err(|e| format!("Failed to set stream to blocking: {}", e))?;
                break stream;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("Helper did not connect within timeout. User may have cancelled password prompt.".to_string());
                }
                // Log every 5 seconds
                thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Failed to accept connection: {}", e));
            }
        }
    };
    
    // Read progress messages from helper using blocking reads
    // Cancel is checked after each line - helper sends progress frequently (~10x/sec)
    stream.set_read_timeout(Some(std::time::Duration::from_secs(60)))
        .map_err(|e| format!("Failed to set read timeout: {}", e))?;
    
    let reader = BufReader::new(stream);
    let mut bytes_written = 0u64;
    let mut checksum = 0u32;
    let mut error: Option<String> = None;
    
    for line in reader.lines() {
        // Check for cancellation after each message
        if CANCEL_FLAG.load(Ordering::SeqCst) {
            let _ = child.kill();
            return Err("Flash cancelled by user".to_string());
        }
        
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error = Some(format!("Connection error: {}", e));
                break;
            }
        };
        
        if line.is_empty() {
            continue;
        }
        
        // Parse JSON message
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            match json.get("type").and_then(|t| t.as_str()) {
                Some("progress") => {
                    let progress = FlashProgress {
                        bytes_written: json.get("bytes_written").and_then(|v| v.as_u64()).unwrap_or(0),
                        total_bytes: json.get("total_bytes").and_then(|v| v.as_u64()).unwrap_or(0),
                        percentage: json.get("percentage").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        speed: json.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        eta: json.get("eta").and_then(|v| v.as_f64()),
                        stage: json.get("stage").and_then(|v| v.as_str()).unwrap_or("flashing").to_string(),
                        active: 1,
                        failed: 0,
                    };
                    let _ = app.emit("flash-progress", &progress);
                }
                Some("result") => {
                    if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        bytes_written = json.get("bytes_written").and_then(|v| v.as_u64()).unwrap_or(0);
                        checksum = json.get("checksum").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    } else {
                        error = json.get("error").and_then(|v| v.as_str()).map(|s| s.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    
    // Wait for child to exit
    let status = child.wait()
        .map_err(|e| format!("Failed to wait for helper: {}", e))?;
    
    if let Some(err) = error {
        return Err(err);
    }
    
    if !status.success() {
        // Read stderr for error message
        if let Some(stderr) = child.stderr.take() {
            let mut stderr_content = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut stderr_content);
            if !stderr_content.is_empty() {
                return Err(stderr_content);
            }
        }
        return Err(format!("Helper exited with code: {:?}", status.code()));
    }
    
    Ok(FlashResult {
        bytes_written,
        devices: DeviceResult {
            successful: 1,
            failed: 0,
        },
        errors: Vec::new(),
        source_checksum: checksum,
    })
}

/// Find the etcher-helper binary
fn find_helper_binary() -> Result<String, String> {
    use std::path::Path;
    
    // Helper binary name
    #[cfg(target_os = "windows")]
    let helper_name = "etcher-helper.exe";
    #[cfg(not(target_os = "windows"))]
    let helper_name = "etcher-helper";
    
    // Get target triple for bundled binary name (used in dev/binaries folder)
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let bundled_name = "etcher-helper-aarch64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    let bundled_name = "etcher-helper-x86_64-apple-darwin";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let bundled_name = "etcher-helper-x86_64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let bundled_name = "etcher-helper-aarch64-unknown-linux-gnu";
    #[cfg(target_os = "windows")]
    let bundled_name = "etcher-helper-x86_64-pc-windows-msvc.exe";
    
    // 1. Check next to the current executable (for bundled production builds)
    // In a .app bundle, both binaries are in Contents/MacOS/
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let sibling_path = exe_dir.join(helper_name);
            if sibling_path.exists() {
                return sibling_path.to_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| "Invalid path".to_string());
            }
        }
    }
    
    // For development builds, use CARGO_MANIFEST_DIR
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    
    // 2. Check bundled binaries directory (for dev with pre-built helper)
    let bundled_path = Path::new(manifest_dir).join("binaries").join(bundled_name);
    if bundled_path.exists() {
        return bundled_path.to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Invalid path".to_string());
    }
    
    // 3. Check debug build
    let debug_path = Path::new(manifest_dir).join("target").join("debug").join(helper_name);
    if debug_path.exists() {
        return debug_path.to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Invalid path".to_string());
    }
    
    // 4. Check release build
    let release_path = Path::new(manifest_dir).join("target").join("release").join(helper_name);
    if release_path.exists() {
        return release_path.to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Invalid path".to_string());
    }
    
    Err(format!(
        "Helper binary not found. Searched next to exe, and in: {:?}, {:?}, {:?}",
        bundled_path, debug_path, release_path
    ))
}

/// Verify the write by reading back and comparing checksums
fn verify_write(
    dest_path: &str,
    expected_bytes: u64,
    expected_checksum: u32,
    app: &AppHandle,
) -> Result<(), String> {
    let mut dest = File::open(dest_path)
        .map_err(|e| format!("Failed to open for verification: {}", e))?;
    
    const BUFFER_SIZE: usize = 1024 * 1024;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut hasher = crc32fast::Hasher::new();
    let mut bytes_read_total: u64 = 0;
    
    let start_time = std::time::Instant::now();
    let mut last_progress_time = start_time;
    let mut last_bytes = 0u64;
    
    loop {
        if CANCEL_FLAG.load(Ordering::SeqCst) {
            return Err("Verification cancelled by user".to_string());
        }
        
        // Only read up to expected_bytes
        let to_read = std::cmp::min(
            BUFFER_SIZE,
            (expected_bytes - bytes_read_total) as usize
        );
        
        if to_read == 0 {
            break;
        }
        
        let bytes_read = dest.read(&mut buffer[..to_read])
            .map_err(|e| format!("Verification read error: {}", e))?;
        
        if bytes_read == 0 {
            break;
        }
        
        hasher.update(&buffer[..bytes_read]);
        bytes_read_total += bytes_read as u64;
        
        // Emit progress
        let now = std::time::Instant::now();
        if now.duration_since(last_progress_time).as_millis() >= 100 {
            let recent_elapsed = now.duration_since(last_progress_time).as_secs_f64();
            let recent_bytes = bytes_read_total - last_bytes;
            
            let speed = if recent_elapsed > 0.0 {
                recent_bytes as f64 / recent_elapsed
            } else {
                0.0
            };
            
            let percentage = (bytes_read_total as f64 / expected_bytes as f64) * 100.0;
            let remaining = expected_bytes - bytes_read_total;
            let eta = if speed > 0.0 {
                Some(remaining as f64 / speed)
            } else {
                None
            };
            
            let progress = FlashProgress {
                bytes_written: bytes_read_total,
                total_bytes: expected_bytes,
                percentage,
                speed,
                eta,
                stage: "verifying".to_string(),
                active: 1,
                failed: 0,
            };
            
            let _ = app.emit("flash-progress", &progress);
            
            last_progress_time = now;
            last_bytes = bytes_read_total;
        }
    }
    
    let actual_checksum = hasher.finalize();
    
    if actual_checksum != expected_checksum {
        return Err(format!(
            "Verification failed: checksum mismatch. Expected {:08x}, got {:08x}",
            expected_checksum, actual_checksum
        ));
    }
    
    if bytes_read_total != expected_bytes {
        return Err(format!(
            "Verification failed: size mismatch. Expected {} bytes, read {} bytes",
            expected_bytes, bytes_read_total
        ));
    }
    
    Ok(())
}

/// Get metadata about a source image
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMetadata {
    pub path: String,
    pub size: Option<u64>,
    pub compressed_size: Option<u64>,
    pub is_compressed: bool,
    pub compression_type: Option<String>,
    pub extension: String,
    pub name: String,
}

#[tauri::command]
pub fn get_source_metadata(path: String) -> Result<SourceMetadata, String> {
    let file = File::open(&path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    let file_size = file.metadata()
        .map(|m| m.len())
        .map_err(|e| format!("Failed to get metadata: {}", e))?;
    
    let compression = detect_compression(&path);
    let is_compressed = compression != Compression::None;
    
    let compression_type = match compression {
        Compression::Gzip => Some("gzip".to_string()),
        Compression::Xz => Some("xz".to_string()),
        Compression::Bz2 => Some("bz2".to_string()),
        Compression::Zip => Some("zip".to_string()),
        Compression::None => None,
    };
    
    // Extract filename and extension
    let path_obj = std::path::Path::new(&path);
    let name = path_obj.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    
    let extension = path_obj.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    
    Ok(SourceMetadata {
        path,
        size: if is_compressed { None } else { Some(file_size) },
        compressed_size: if is_compressed { Some(file_size) } else { None },
        is_compressed,
        compression_type,
        extension,
        name,
    })
}
