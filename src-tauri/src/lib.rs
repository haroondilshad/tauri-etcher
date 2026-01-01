use std::path::PathBuf;
use tauri::Manager;

mod drives;
mod flash;

/// Get the path to the etcher-util sidecar binary
#[tauri::command]
fn get_sidecar_path(app: tauri::AppHandle) -> Result<String, String> {
    let sidecar_name = if cfg!(target_os = "windows") {
        "etcher-util.exe"
    } else {
        "etcher-util"
    };

    // In development, look for the sidecar in the out/sidecar/bin directory
    // Try multiple possible paths
    if cfg!(debug_assertions) {
        // Get the manifest directory at compile time (where Cargo.toml is)
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let project_root = std::path::Path::new(manifest_dir).parent()
            .ok_or_else(|| "Cannot determine project root".to_string())?;
        
        // Check the out/sidecar/bin directory relative to project root
        let dev_path = project_root.join("out").join("sidecar").join("bin").join(sidecar_name);
        println!("Checking dev sidecar path: {:?}", dev_path);
        
        if dev_path.exists() {
            return dev_path.to_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Invalid path".to_string());
        }
        
        // Also check relative to current working directory
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let cwd_path = cwd.join("out").join("sidecar").join("bin").join(sidecar_name);
        println!("Checking cwd sidecar path: {:?}", cwd_path);
        
        if cwd_path.exists() {
            return cwd_path.to_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Invalid path".to_string());
        }
        
        // Check parent of current dir (in case we're in src-tauri)
        let parent_path = cwd.parent()
            .map(|p| p.join("out").join("sidecar").join("bin").join(sidecar_name));
        
        if let Some(ref path) = parent_path {
            println!("Checking parent sidecar path: {:?}", path);
            if path.exists() {
                return path.to_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| "Invalid path".to_string());
            }
        }
        
        println!("Dev sidecar not found at any expected location");
    }

    // In production, the sidecar is bundled with the app
    let resource_path = app.path()
        .resource_dir()
        .map_err(|e| e.to_string())?;
    
    let sidecar_path: PathBuf = resource_path.join("binaries").join(sidecar_name);
    println!("Using resource sidecar path: {:?}", sidecar_path);
    
    sidecar_path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Invalid sidecar path".to_string())
}

/// Get the path to the sidecar JavaScript file (for running with Node.js directly)
/// This bypasses the pkg bundler which has issues with WASM modules like ext2fs
#[tauri::command]
fn get_sidecar_script_path() -> Result<String, String> {
    // Get the manifest directory at compile time (where Cargo.toml is)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let project_root = std::path::Path::new(manifest_dir).parent()
        .ok_or_else(|| "Cannot determine project root".to_string())?;
    
    // The compiled JS sidecar is at out/sidecar/src/util/api.js
    let script_path = project_root.join("out").join("sidecar").join("src").join("util").join("api.js");
    println!("Sidecar script path: {:?}", script_path);
    
    if script_path.exists() {
        return script_path.to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Invalid path".to_string());
    }
    
    // Also check CWD
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let cwd_path = cwd.join("out").join("sidecar").join("src").join("util").join("api.js");
    println!("Checking cwd script path: {:?}", cwd_path);
    
    if cwd_path.exists() {
        return cwd_path.to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Invalid path".to_string());
    }
    
    Err("Sidecar script not found. Run 'tsc --project tsconfig.sidecar.json --outDir out/sidecar/src' first.".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_sidecar_path, 
            get_sidecar_script_path,
            drives::list_drives,
            drives::unmount_disk,
            flash::flash_image,
            flash::cancel_flash,
            flash::get_source_metadata
        ])
        .setup(|app| {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                use tauri_plugin_single_instance::init as single_instance_init;
                app.handle().plugin(single_instance_init(|_app, argv, _cwd| {
                    // Handle second instance - could send image URL to main window
                    println!("Second instance detected with args: {:?}", argv);
                }))?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
