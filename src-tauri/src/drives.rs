/*
 * Drive scanning module
 * 
 * Replaces the Node.js drivelist module with native Rust implementation.
 * Uses platform-specific commands to enumerate drives.
 */

use serde::{Deserialize, Serialize};
use std::process::Command;

/// Mountpoint information for a drive
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mountpoint {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Drive information matching the drivelist interface expected by the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub device: String,           // /dev/disk4
    pub raw: String,              // /dev/rdisk4 (faster raw device on macOS)
    pub description: String,      // "USB Flash Drive"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,        // Size in bytes
    pub mountpoints: Vec<Mountpoint>,
    pub is_system: bool,
    pub is_read_only: bool,
    pub is_removable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_usb: Option<bool>,
    pub block_size: u64,
    pub bus_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_path: Option<String>,
    // Additional fields for frontend compatibility
    pub disabled: bool,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    pub display_name: String,
}

/// List all available drives
/// 
/// Returns drives suitable for flashing (filters out system drives by default display,
/// but includes them for user awareness with warnings)
#[tauri::command]
pub fn list_drives() -> Result<Vec<DriveInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        list_drives_macos()
    }
    
    #[cfg(target_os = "linux")]
    {
        list_drives_linux()
    }
    
    #[cfg(target_os = "windows")]
    {
        list_drives_windows()
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported platform".to_string())
    }
}

/// macOS implementation using diskutil
#[cfg(target_os = "macos")]
fn list_drives_macos() -> Result<Vec<DriveInfo>, String> {
    // Run diskutil list -plist to get all disks
    let output = Command::new("diskutil")
        .args(["list", "-plist"])
        .output()
        .map_err(|e| format!("Failed to run diskutil: {}", e))?;
    
    if !output.status.success() {
        return Err(format!(
            "diskutil failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    let plist_str = String::from_utf8_lossy(&output.stdout);
    
    // Parse the plist to extract disk names
    let disk_names = parse_diskutil_list(&plist_str)?;
    
    // Get detailed info for each disk
    let mut drives = Vec::new();
    for disk_name in disk_names {
        if let Ok(drive) = get_disk_info_macos(&disk_name) {
            drives.push(drive);
        }
    }
    
    Ok(drives)
}

/// Parse diskutil list plist output to extract disk identifiers
#[cfg(target_os = "macos")]
fn parse_diskutil_list(plist: &str) -> Result<Vec<String>, String> {
    // Simple regex-free parsing for AllDisks array
    // Format: <key>AllDisks</key><array><string>disk0</string>...
    let mut disks = Vec::new();
    
    // Find AllDisks section
    if let Some(start) = plist.find("<key>AllDisks</key>") {
        let rest = &plist[start..];
        if let Some(array_start) = rest.find("<array>") {
            if let Some(array_end) = rest.find("</array>") {
                let array_content = &rest[array_start..array_end];
                
                // Extract <string>diskN</string> entries
                let mut pos = 0;
                while let Some(str_start) = array_content[pos..].find("<string>") {
                    let actual_start = pos + str_start + 8; // len("<string>")
                    if let Some(str_end) = array_content[actual_start..].find("</string>") {
                        let disk_name = &array_content[actual_start..actual_start + str_end];
                        disks.push(disk_name.to_string());
                        pos = actual_start + str_end;
                    } else {
                        break;
                    }
                }
            }
        }
    }
    
    // Filter to only whole disks (e.g., "disk0", "disk1", not "disk0s1")
    // A whole disk is one where after "disk" there's only a number
    let whole_disks: Vec<String> = disks
        .into_iter()
        .filter(|d| {
            if !d.starts_with("disk") {
                return false;
            }
            let after_disk = &d[4..];
            // Whole disk: all characters after "disk" are digits
            after_disk.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    
    Ok(whole_disks)
}

/// Get detailed info for a specific disk on macOS
#[cfg(target_os = "macos")]
fn get_disk_info_macos(disk_name: &str) -> Result<DriveInfo, String> {
    let output = Command::new("diskutil")
        .args(["info", "-plist", disk_name])
        .output()
        .map_err(|e| format!("Failed to get disk info: {}", e))?;
    
    if !output.status.success() {
        return Err(format!("diskutil info failed for {}", disk_name));
    }
    
    let plist_str = String::from_utf8_lossy(&output.stdout);
    parse_disk_info_plist(&plist_str, disk_name)
}

/// Parse diskutil info plist output
#[cfg(target_os = "macos")]
fn parse_disk_info_plist(plist: &str, disk_name: &str) -> Result<DriveInfo, String> {
    let device = format!("/dev/{}", disk_name);
    let raw = format!("/dev/r{}", disk_name);
    
    // Helper to extract string value from plist
    let get_string = |key: &str| -> Option<String> {
        let key_tag = format!("<key>{}</key>", key);
        if let Some(key_pos) = plist.find(&key_tag) {
            let rest = &plist[key_pos + key_tag.len()..];
            if let Some(start) = rest.find("<string>") {
                let value_start = start + 8;
                if let Some(end) = rest[value_start..].find("</string>") {
                    return Some(rest[value_start..value_start + end].to_string());
                }
            }
        }
        None
    };
    
    // Helper to extract integer value from plist
    let get_integer = |key: &str| -> Option<u64> {
        let key_tag = format!("<key>{}</key>", key);
        if let Some(key_pos) = plist.find(&key_tag) {
            let rest = &plist[key_pos + key_tag.len()..];
            if let Some(start) = rest.find("<integer>") {
                let value_start = start + 9;
                if let Some(end) = rest[value_start..].find("</integer>") {
                    return rest[value_start..value_start + end].parse().ok();
                }
            }
        }
        None
    };
    
    // Helper to check boolean value from plist
    let get_bool = |key: &str| -> bool {
        let key_tag = format!("<key>{}</key>", key);
        if let Some(key_pos) = plist.find(&key_tag) {
            let rest = &plist[key_pos + key_tag.len()..];
            let next_100 = &rest[..std::cmp::min(100, rest.len())];
            return next_100.contains("<true/>");
        }
        false
    };
    
    let description = get_string("MediaName")
        .or_else(|| get_string("IORegistryEntryName"))
        .unwrap_or_else(|| "Unknown Drive".to_string());
    
    let size = get_integer("TotalSize").or_else(|| get_integer("Size"));
    let block_size = get_integer("DeviceBlockSize").unwrap_or(512);
    
    let is_removable = get_bool("Removable") || get_bool("RemovableMedia");
    let is_internal = get_bool("Internal");
    let is_system = !is_removable && is_internal;
    let is_read_only = get_bool("WritableMedia") == false && plist.contains("<key>WritableMedia</key>");
    
    let bus_type = get_string("BusProtocol").unwrap_or_else(|| "Unknown".to_string());
    let is_usb = bus_type.to_lowercase().contains("usb");
    
    // Get mount points
    let mount_point = get_string("MountPoint");
    let mountpoints = if let Some(mp) = mount_point {
        if !mp.is_empty() {
            vec![Mountpoint {
                path: mp,
                label: get_string("VolumeName"),
            }]
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    
    let display_name = format!(
        "{} - {}",
        description,
        size.map(|s| format_size(s)).unwrap_or_else(|| "Unknown size".to_string())
    );
    
    Ok(DriveInfo {
        device: device.clone(),
        raw,
        description: description.clone(),
        size,
        mountpoints,
        is_system,
        is_read_only,
        is_removable,
        is_usb: Some(is_usb),
        block_size,
        bus_type,
        device_path: Some(device.clone()),
        disabled: false,
        name: description.clone(),
        path: device,
        logo: None,
        display_name,
    })
}

/// Format size in human-readable format
fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;
    
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Linux implementation using lsblk
#[cfg(target_os = "linux")]
fn list_drives_linux() -> Result<Vec<DriveInfo>, String> {
    let output = Command::new("lsblk")
        .args(["-J", "-b", "-o", "NAME,SIZE,TYPE,MOUNTPOINT,RM,RO,MODEL,TRAN,LABEL"])
        .output()
        .map_err(|e| format!("Failed to run lsblk: {}", e))?;
    
    if !output.status.success() {
        return Err(format!(
            "lsblk failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    let json_str = String::from_utf8_lossy(&output.stdout);
    parse_lsblk_json(&json_str)
}

#[cfg(target_os = "linux")]
fn parse_lsblk_json(json_str: &str) -> Result<Vec<DriveInfo>, String> {
    // Parse JSON output from lsblk
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse lsblk output: {}", e))?;
    
    let mut drives = Vec::new();
    
    if let Some(blockdevices) = parsed.get("blockdevices").and_then(|v| v.as_array()) {
        for device in blockdevices {
            // Only include disk type devices
            let device_type = device.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if device_type != "disk" {
                continue;
            }
            
            let name = device.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let device_path = format!("/dev/{}", name);
            
            let size = device.get("size").and_then(|v| v.as_u64());
            let is_removable = device.get("rm").and_then(|v| v.as_bool()).unwrap_or(false);
            let is_read_only = device.get("ro").and_then(|v| v.as_bool()).unwrap_or(false);
            let model = device.get("model").and_then(|v| v.as_str()).unwrap_or("Unknown");
            let transport = device.get("tran").and_then(|v| v.as_str()).unwrap_or("Unknown");
            
            // Collect mountpoints from partitions
            let mut mountpoints = Vec::new();
            if let Some(children) = device.get("children").and_then(|v| v.as_array()) {
                for child in children {
                    if let Some(mp) = child.get("mountpoint").and_then(|v| v.as_str()) {
                        if !mp.is_empty() {
                            mountpoints.push(Mountpoint {
                                path: mp.to_string(),
                                label: child.get("label").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            });
                        }
                    }
                }
            }
            
            let is_system = !is_removable && mountpoints.iter().any(|mp| mp.path == "/");
            let is_usb = transport.to_lowercase() == "usb";
            
            let display_name = format!(
                "{} - {}",
                model,
                size.map(|s| format_size(s)).unwrap_or_else(|| "Unknown size".to_string())
            );
            
            drives.push(DriveInfo {
                device: device_path.clone(),
                raw: device_path.clone(), // Linux doesn't have separate raw devices
                description: model.to_string(),
                size,
                mountpoints,
                is_system,
                is_read_only,
                is_removable,
                is_usb: Some(is_usb),
                block_size: 512, // Default, could be read from /sys
                bus_type: transport.to_string(),
                device_path: Some(device_path.clone()),
                disabled: false,
                name: model.to_string(),
                path: device_path,
                logo: None,
                display_name,
            });
        }
    }
    
    Ok(drives)
}

/// Windows implementation using PowerShell/WMI
#[cfg(target_os = "windows")]
fn list_drives_windows() -> Result<Vec<DriveInfo>, String> {
    // Use PowerShell to get disk information via CIM
    let script = r#"
        Get-CimInstance -ClassName Win32_DiskDrive | ForEach-Object {
            $disk = $_
            $partitions = Get-CimAssociatedInstance -InputObject $disk -ResultClassName Win32_DiskPartition
            $mountpoints = @()
            foreach ($partition in $partitions) {
                $logicalDisks = Get-CimAssociatedInstance -InputObject $partition -ResultClassName Win32_LogicalDisk
                foreach ($logicalDisk in $logicalDisks) {
                    $mountpoints += @{
                        path = $logicalDisk.DeviceID
                        label = $logicalDisk.VolumeName
                    }
                }
            }
            @{
                device = $disk.DeviceID
                description = $disk.Model
                size = $disk.Size
                isRemovable = $disk.MediaType -match 'Removable'
                isUsb = $disk.InterfaceType -eq 'USB'
                busType = $disk.InterfaceType
                mountpoints = $mountpoints
            }
        } | ConvertTo-Json -Depth 3
    "#;
    
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| format!("Failed to run PowerShell: {}", e))?;
    
    if !output.status.success() {
        return Err(format!(
            "PowerShell failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    let json_str = String::from_utf8_lossy(&output.stdout);
    parse_windows_json(&json_str)
}

#[cfg(target_os = "windows")]
fn parse_windows_json(json_str: &str) -> Result<Vec<DriveInfo>, String> {
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse PowerShell output: {}", e))?;
    
    let mut drives = Vec::new();
    
    // Handle both single object and array responses
    let devices: Vec<&serde_json::Value> = if parsed.is_array() {
        parsed.as_array().unwrap().iter().collect()
    } else {
        vec![&parsed]
    };
    
    for device in devices {
        let device_id = device.get("device").and_then(|v| v.as_str()).unwrap_or("");
        let description = device.get("description").and_then(|v| v.as_str()).unwrap_or("Unknown");
        let size = device.get("size").and_then(|v| v.as_u64());
        let is_removable = device.get("isRemovable").and_then(|v| v.as_bool()).unwrap_or(false);
        let is_usb = device.get("isUsb").and_then(|v| v.as_bool()).unwrap_or(false);
        let bus_type = device.get("busType").and_then(|v| v.as_str()).unwrap_or("Unknown");
        
        let mut mountpoints = Vec::new();
        if let Some(mps) = device.get("mountpoints").and_then(|v| v.as_array()) {
            for mp in mps {
                if let Some(path) = mp.get("path").and_then(|v| v.as_str()) {
                    mountpoints.push(Mountpoint {
                        path: path.to_string(),
                        label: mp.get("label").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    });
                }
            }
        }
        
        // System drive typically has C:
        let is_system = mountpoints.iter().any(|mp| mp.path.to_uppercase().starts_with("C:"));
        
        let display_name = format!(
            "{} - {}",
            description,
            size.map(|s| format_size(s)).unwrap_or_else(|| "Unknown size".to_string())
        );
        
        drives.push(DriveInfo {
            device: device_id.to_string(),
            raw: device_id.to_string(),
            description: description.to_string(),
            size,
            mountpoints,
            is_system,
            is_read_only: false,
            is_removable,
            is_usb: Some(is_usb),
            block_size: 512,
            bus_type: bus_type.to_string(),
            device_path: Some(device_id.to_string()),
            disabled: false,
            name: description.to_string(),
            path: device_id.to_string(),
            logo: None,
            display_name,
        });
    }
    
    Ok(drives)
}

/// Unmount a disk before writing
#[tauri::command]
pub fn unmount_disk(device: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("diskutil")
            .args(["unmountDisk", &device])
            .output()
            .map_err(|e| format!("Failed to unmount: {}", e))?;
        
        if !output.status.success() {
            // It's okay if unmount fails because disk wasn't mounted
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("not mounted") {
                return Err(format!("Failed to unmount {}: {}", device, stderr));
            }
        }
        Ok(())
    }
    
    #[cfg(target_os = "linux")]
    {
        // On Linux, unmount each partition
        let output = Command::new("umount")
            .args(["-f", &format!("{}*", device)])
            .output();
        
        // Ignore errors, partition might not be mounted
        let _ = output;
        Ok(())
    }
    
    #[cfg(target_os = "windows")]
    {
        // On Windows, use diskpart or PowerShell
        // For now, just return Ok - Windows handles this differently
        let _ = device; // Suppress unused variable warning
        Ok(())
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported platform".to_string())
    }
}
