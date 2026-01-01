/*!
 * Etcher Helper - Privileged disk writing helper
 * 
 * This is a separate binary that runs with elevated privileges to write
 * images to disk. It communicates with the main app via a local socket.
 * 
 * Usage:
 *   etcher-helper <source_path> <dest_device> <socket_port> [--verify]
 */

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;

use flate2::read::GzDecoder;
use xz2::read::XzDecoder;
use bzip2::read::BzDecoder;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ProgressMessage {
    #[serde(rename = "type")]
    msg_type: String,
    bytes_written: u64,
    total_bytes: u64,
    percentage: f64,
    speed: f64,
    eta: Option<f64>,
    stage: String,
}

#[derive(Debug, Serialize)]
struct ResultMessage {
    #[serde(rename = "type")]
    msg_type: String,
    success: bool,
    bytes_written: u64,
    checksum: u32,
    error: Option<String>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 4 {
        eprintln!("Usage: etcher-helper <source_path> <dest_device> <socket_port> [--verify]");
        std::process::exit(1);
    }
    
    let source_path = &args[1];
    let dest_device = &args[2];
    let socket_port: u16 = args[3].parse().unwrap_or(0);
    let verify = args.get(4).map(|s| s == "--verify").unwrap_or(false);
    
    if socket_port == 0 {
        eprintln!("Invalid socket port");
        std::process::exit(1);
    }
    
    // Connect to the main app
    let mut socket = match TcpStream::connect(format!("127.0.0.1:{}", socket_port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to connect to main app: {}", e);
            std::process::exit(1);
        }
    };
    
    // Run the flash operation
    match flash_image(source_path, dest_device, verify, &mut socket) {
        Ok((bytes_written, checksum)) => {
            let result = ResultMessage {
                msg_type: "result".to_string(),
                success: true,
                bytes_written,
                checksum,
                error: None,
            };
            let _ = writeln!(socket, "{}", serde_json::to_string(&result).unwrap());
        }
        Err(e) => {
            let result = ResultMessage {
                msg_type: "result".to_string(),
                success: false,
                bytes_written: 0,
                checksum: 0,
                error: Some(e),
            };
            let _ = writeln!(socket, "{}", serde_json::to_string(&result).unwrap());
            std::process::exit(1);
        }
    }
}

fn flash_image(
    source_path: &str,
    dest_device: &str,
    verify: bool,
    socket: &mut TcpStream,
) -> Result<(u64, u32), String> {
    // Unmount the disk first
    unmount_disk(dest_device)?;
    
    // Open source with automatic decompression
    let (mut source, known_size) = open_source(source_path)?;
    
    // Open destination for writing (use raw device on macOS)
    let dest_path = if cfg!(target_os = "macos") && dest_device.contains("/dev/disk") {
        dest_device.replace("/dev/disk", "/dev/rdisk")
    } else {
        dest_device.to_string()
    };
    
    let mut dest = OpenOptions::new()
        .write(true)
        .open(&dest_path)
        .map_err(|e| format!("Failed to open {}: {}", dest_path, e))?;
    
    // Buffer for reading/writing
    const BUFFER_SIZE: usize = 1024 * 1024;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    
    let mut bytes_written: u64 = 0;
    let mut hasher = crc32fast::Hasher::new();
    let start_time = std::time::Instant::now();
    let mut last_progress_time = start_time;
    let mut last_bytes = 0u64;
    
    // Flash loop
    loop {
        let bytes_read = source.read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;
        
        if bytes_read == 0 {
            break;
        }
        
        dest.write_all(&buffer[..bytes_read])
            .map_err(|e| format!("Write error: {}", e))?;
        
        hasher.update(&buffer[..bytes_read]);
        bytes_written += bytes_read as u64;
        
        // Send progress every 100ms
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
                (0.0, None)
            };
            
            let progress = ProgressMessage {
                msg_type: "progress".to_string(),
                bytes_written,
                total_bytes: known_size.unwrap_or(0),
                percentage,
                speed,
                eta,
                stage: "flashing".to_string(),
            };
            
            let _ = writeln!(socket, "{}", serde_json::to_string(&progress).unwrap());
            
            last_progress_time = now;
            last_bytes = bytes_written;
        }
    }
    
    // sync_all() may fail on raw block devices (macOS returns ENOTTY)
    // This is safe to ignore as the data has been written
    if let Err(e) = dest.sync_all() {
        eprintln!("Note: sync_all failed (this is normal for raw devices): {}", e);
    }
    
    let checksum = hasher.finalize();
    
    // Verification phase
    if verify {
        verify_write(&dest_path, bytes_written, checksum, socket)?;
    }
    
    Ok((bytes_written, checksum))
}

fn verify_write(
    dest_path: &str,
    expected_bytes: u64,
    expected_checksum: u32,
    socket: &mut TcpStream,
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
            
            let progress = ProgressMessage {
                msg_type: "progress".to_string(),
                bytes_written: bytes_read_total,
                total_bytes: expected_bytes,
                percentage,
                speed,
                eta,
                stage: "verifying".to_string(),
            };
            
            let _ = writeln!(socket, "{}", serde_json::to_string(&progress).unwrap());
            
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
    
    Ok(())
}

fn unmount_disk(device: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("diskutil")
            .args(["unmountDisk", device])
            .output()
            .map_err(|e| format!("Failed to unmount: {}", e))?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("not mounted") {
                return Err(format!("Failed to unmount: {}", stderr));
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let _ = Command::new("umount")
            .args(["-f", device])
            .output();
    }
    
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum Compression {
    None,
    Gzip,
    Xz,
    Bz2,
    Zip,
}

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

fn open_source(path: &str) -> Result<(Box<dyn Read + Send>, Option<u64>), String> {
    let file = File::open(path).map_err(|e| format!("Failed to open source: {}", e))?;
    let file_size = file.metadata().map(|m| m.len()).ok();
    
    match detect_compression(path) {
        Compression::Gzip => {
            let decoder = GzDecoder::new(BufReader::new(file));
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
            let reader = BufReader::new(file);
            let zip_reader = open_zip_first_file(reader)?;
            Ok((zip_reader, None))
        }
        Compression::None => {
            Ok((Box::new(BufReader::new(file)), file_size))
        }
    }
}

fn open_zip_first_file(reader: BufReader<File>) -> Result<Box<dyn Read + Send>, String> {
    use std::io::Seek;
    
    let mut file = reader.into_inner();
    file.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| format!("Failed to seek: {}", e))?;
    
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to open ZIP: {}", e))?;
    
    if archive.is_empty() {
        return Err("ZIP archive is empty".to_string());
    }
    
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
    
    let mut zip_file = archive.by_index(target_index)
        .map_err(|e| format!("Failed to extract from ZIP: {}", e))?;
    
    let mut buffer = Vec::new();
    zip_file.read_to_end(&mut buffer)
        .map_err(|e| format!("Failed to read ZIP contents: {}", e))?;
    
    Ok(Box::new(std::io::Cursor::new(buffer)))
}
