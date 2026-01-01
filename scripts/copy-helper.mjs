#!/usr/bin/env node

/**
 * Copy the etcher-helper binary to the correct location for Tauri bundling.
 * 
 * Tauri expects external binaries to be named with the target triple suffix:
 * - macOS Intel: etcher-helper-x86_64-apple-darwin
 * - macOS ARM: etcher-helper-aarch64-apple-darwin
 * - Linux: etcher-helper-x86_64-unknown-linux-gnu
 * - Windows: etcher-helper-x86_64-pc-windows-msvc.exe
 * 
 * This script also creates a placeholder if no binary exists yet, to allow
 * the initial cargo build to succeed (Tauri's build script checks for the file).
 */

import { copyFileSync, existsSync, mkdirSync, writeFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(__dirname, '..');
const srcTauri = join(projectRoot, 'src-tauri');
const binariesDir = join(srcTauri, 'binaries');

// Ensure binaries directory exists
if (!existsSync(binariesDir)) {
    mkdirSync(binariesDir, { recursive: true });
}

// Get the current target triple
function getTargetTriple() {
    const platform = process.platform;
    const arch = process.arch;
    
    if (platform === 'darwin') {
        return arch === 'arm64' 
            ? 'aarch64-apple-darwin'
            : 'x86_64-apple-darwin';
    } else if (platform === 'linux') {
        return arch === 'arm64'
            ? 'aarch64-unknown-linux-gnu'
            : 'x86_64-unknown-linux-gnu';
    } else if (platform === 'win32') {
        return 'x86_64-pc-windows-msvc';
    }
    
    throw new Error(`Unsupported platform: ${platform}`);
}

// Get the binary extension for the current platform
function getBinaryExtension() {
    return process.platform === 'win32' ? '.exe' : '';
}

const targetTriple = getTargetTriple();
const ext = getBinaryExtension();

const sourcePath = join(srcTauri, 'target', 'release', `etcher-helper${ext}`);
const destPath = join(binariesDir, `etcher-helper-${targetTriple}${ext}`);

// Check if --placeholder flag is passed (for initial bootstrap)
const isPlaceholder = process.argv.includes('--placeholder');

if (isPlaceholder) {
    if (!existsSync(destPath)) {
        console.log(`Creating placeholder at: ${destPath}`);
        writeFileSync(destPath, '');
        console.log('Placeholder created. Run npm run build:helper to build the real binary.');
    } else {
        console.log('Helper binary or placeholder already exists.');
    }
} else {
    if (!existsSync(sourcePath)) {
        console.error(`Error: Helper binary not found at ${sourcePath}`);
        console.error('Please build it first with: cd src-tauri && cargo build --release --bin etcher-helper');
        process.exit(1);
    }

    console.log(`Copying helper binary:`);
    console.log(`  From: ${sourcePath}`);
    console.log(`  To: ${destPath}`);

    copyFileSync(sourcePath, destPath);

    // Make the binary executable on Unix
    if (process.platform !== 'win32') {
        execSync(`chmod +x "${destPath}"`);
    }

    console.log('Done!');
}
