/**
 * Native Rust API module
 * 
 * This replaces the Node.js sidecar WebSocket API with direct Tauri invoke calls.
 * All drive scanning, flashing, and metadata operations are handled by Rust.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type { DrivelistDrive } from '../../../shared/drive-constraints';
import type { SourceMetadata } from '../../../shared/typings/source-selector';

// Types matching Rust structs
export interface DriveInfo {
	device: string;
	raw: string;
	description: string;
	size: number | null;
	mountpoints: { path: string; label: string | null }[];
	isSystem: boolean;
	isReadOnly: boolean;
	isRemovable: boolean;
	isUsb: boolean | null;
	blockSize: number;
	busType: string;
	devicePath: string | null;
	disabled: boolean;
	name: string;
	path: string;
	logo: string | null;
	displayName: string;
}

export interface FlashProgress {
	bytesWritten: number;
	totalBytes: number;
	percentage: number;
	speed: number;
	eta: number | null;
	stage: 'flashing' | 'verifying';
	active: number;
	failed: number;
}

export interface FlashResult {
	bytesWritten: number;
	devices: {
		successful: number;
		failed: number;
	};
	errors: string[];
	sourceChecksum: number;
}

export interface RustSourceMetadata {
	path: string;
	size: number | null;
	compressedSize: number | null;
	isCompressed: boolean;
	compressionType: string | null;
	extension: string;
	name: string;
}

/**
 * List all available drives
 */
export async function listDrives(): Promise<DriveInfo[]> {
	try {
		const drives = await invoke<DriveInfo[]>('list_drives');
		return drives;
	} catch (error) {
		console.error('Error calling list_drives:', error);
		return [];
	}
}

/**
 * Get metadata about a source image
 */
export async function getSourceMetadata(path: string): Promise<RustSourceMetadata> {
	return await invoke<RustSourceMetadata>('get_source_metadata', { path });
}

/**
 * Flash an image to a drive
 * 
 * @param sourcePath - Path to the source image file
 * @param destDevice - Destination device path (e.g., /dev/disk4)
 * @param verify - Whether to verify the write
 * @param onProgress - Callback for progress updates
 */
export async function flashImage(
	sourcePath: string,
	destDevice: string,
	verify: boolean,
	onProgress?: (progress: FlashProgress) => void
): Promise<FlashResult> {
	let unlisten: UnlistenFn | null = null;
	
	try {
		// Set up progress listener before starting flash
		if (onProgress) {
			unlisten = await listen<FlashProgress>('flash-progress', (event) => {
				onProgress(event.payload);
			});
		}
		
		// Invoke the flash command
		const result = await invoke<FlashResult>('flash_image', {
			sourcePath,
			destDevice,
			verify,
		});
		
		return result;
	} finally {
		// Clean up listener
		if (unlisten) {
			unlisten();
		}
	}
}

/**
 * Cancel the current flash operation
 */
export async function cancelFlash(): Promise<void> {
	await invoke('cancel_flash');
}

/**
 * Unmount a disk
 */
export async function unmountDisk(device: string): Promise<void> {
	await invoke('unmount_disk', { device });
}

/**
 * Convert DriveInfo to DrivelistDrive format expected by the frontend
 */
export function toDrivelistDrive(drive: DriveInfo): DrivelistDrive {
	return {
		blockSize: drive.blockSize,
		busType: drive.busType,
		busVersion: null,
		description: drive.description,
		device: drive.device,
		devicePath: drive.devicePath,
		enumerator: 'rust',
		error: null,
		isCard: null,
		isReadOnly: drive.isReadOnly,
		isRemovable: drive.isRemovable,
		isSCSI: null,
		isSystem: drive.isSystem,
		isUAS: null,
		isUSB: drive.isUsb,
		isVirtual: null,
		logicalBlockSize: drive.blockSize,
		mountpoints: drive.mountpoints.map(mp => ({
			path: mp.path,
			label: mp.label,
		})),
		raw: drive.raw,
		size: drive.size,
		partitionTableType: null,
		// Extended properties for DrivelistDrive
		disabled: drive.disabled,
		name: drive.name,
		path: drive.path,
		logo: drive.logo || '',
		displayName: drive.displayName,
	} as DrivelistDrive;
}

/**
 * Convert RustSourceMetadata to SourceMetadata format expected by the frontend
 */
export function toSourceMetadata(meta: RustSourceMetadata, sourcePath: string): Partial<SourceMetadata> {
	return {
		path: meta.path,
		size: meta.size ?? undefined,
		compressedSize: meta.compressedSize ?? undefined,
		isCompressed: meta.isCompressed,
		extension: meta.extension,
		name: meta.name,
		// These would need to be set by the caller
		SourceType: 'File',
		// Skip partition table validation in Tauri mode
		// We don't have etcher-sdk's partition table parsing, and common images (ISO, img) are bootable
		hasMBR: true,
	};
}

/**
 * Start polling for drives
 * Returns a function to stop polling
 */
export function startDrivePolling(
	onDrives: (drives: DrivelistDrive[]) => void,
	intervalMs: number = 2000
): () => void {
	let active = true;
	
	const poll = async () => {
		if (!active) return;
		
		try {
			const drives = await listDrives();
			if (active && drives.length > 0) {
				const converted = drives.map(toDrivelistDrive);
				onDrives(converted);
			}
		} catch (error) {
			console.error('Error polling drives:', error);
		}
		
		if (active) {
			setTimeout(poll, intervalMs);
		}
	};
	
	// Start polling
	poll();
	
	// Return stop function
	return () => {
		active = false;
	};
}
