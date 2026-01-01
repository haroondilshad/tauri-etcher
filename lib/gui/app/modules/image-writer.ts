/*
 * Copyright 2016 balena.io
 * Copyright 2024 - Tauri Migration (Rust-native flashing)
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

import type { Drive as DrivelistDrive } from 'drivelist';
import * as errors from '../../../shared/errors';
import type { SourceMetadata } from '../../../shared/typings/source-selector';
import * as flashState from '../models/flash-state';
import type { FlashProgressState } from '../models/flash-state';
import * as windowProgress from '../os/window-progress';
import { flashImage, cancelFlash, FlashProgress } from './api-rust';

interface FlashResults {
	skip?: boolean;
	cancelled?: boolean;
	results?: {
		bytesWritten: number;
		devices: {
			failed: number;
			successful: number;
		};
		errors: Error[];
	};
}

// Progress handler type
type OnProgressFunction = (state: FlashProgressState) => void;

async function performWrite(
	image: SourceMetadata,
	drives: DrivelistDrive[],
	onProgress: OnProgressFunction,
): Promise<{ cancelled?: boolean }> {
	const flashResults: FlashResults = {};

	// For now, we only support flashing to a single drive
	// Multi-destination can be added later
	if (drives.length === 0) {
		throw errors.createUserError({
			title: 'No target drive selected',
			description: 'Please select a drive to flash the image to.',
		});
	}

	const targetDrive = drives[0];
	const sourcePath = image.path;

	if (!sourcePath) {
		throw errors.createUserError({
			title: 'No source image selected',
			description: 'Please select an image file to flash.',
		});
	}

	console.log('Starting native Rust flash:', { sourcePath, targetDrive: targetDrive.device });

	try {
		// Use the native Rust flash implementation
		const result = await flashImage(
			sourcePath,
			targetDrive.device,
			true, // always verify
			(progress: FlashProgress) => {
				// Convert Rust progress to etcher-sdk compatible format
				// Note: speed is in bytes/s from Rust, flash-state.ts will convert to MB/s
				onProgress({
					type: progress.stage,
					percentage: progress.percentage,
					eta: progress.eta ?? 0,
					speed: progress.speed, // Keep in bytes/s, flash-state.ts converts to MB/s
					active: progress.active,
					failed: progress.failed,
					bytesWritten: progress.bytesWritten,
					bytes: progress.bytesWritten,
					position: progress.bytesWritten,
				});
			}
		);

		console.log('Flash completed:', result);

		flashResults.results = {
			bytesWritten: result.bytesWritten,
			devices: {
				successful: result.devices.successful,
				failed: result.devices.failed,
			},
			errors: result.errors.map((errMsg) => new Error(errMsg)),
		};

		return flashResults;
	} catch (error: any) {
		console.error('Flash error details:', error);
		const errorMessage = typeof error === 'string' ? error : (error.message || JSON.stringify(error));
		
		if (errorMessage?.includes('cancelled')) {
			flashResults.cancelled = true;
			return flashResults;
		}
		
		// Check for permission errors
		if (errorMessage?.includes('Permission denied') || errorMessage?.includes('Operation not permitted') || errorMessage?.includes('elevated privileges')) {
			throw errors.createUserError({
				title: 'Permission Required',
				description: 'Writing to disk requires elevated privileges. On macOS, you may need to grant Full Disk Access to the app or run with sudo.',
			});
		}
		
		throw errors.createUserError({
			title: 'Flash failed',
			description: errorMessage || 'An unknown error occurred during flashing.',
		});
	}
}

/**
 * @summary Flash an image to drives
 */
export async function flash(
	image: SourceMetadata,
	drives: DrivelistDrive[],
	// This function is a parameter so it can be mocked in tests
	write = performWrite,
): Promise<void> {
	if (flashState.isFlashing()) {
		throw new Error('There is already a flash in progress');
	}

	await flashState.setFlashingFlag();

	flashState.setDevicePaths(
		drives.map((d) => d.devicePath).filter((p) => p != null) as string[],
	);

	// start api and call the flasher
	try {
		const result = await write(image, drives, flashState.setProgressState);
		console.log('got results', result);
		await flashState.unsetFlashingFlag(result);
		console.log('removed flashing flag');
	} catch (error: any) {
		await flashState.unsetFlashingFlag({
			cancelled: false,
			errorCode: error.code,
		});

		windowProgress.clear();

		throw error;
	}

	windowProgress.clear();
}

/**
 * @summary Cancel write operation
 */
export async function cancel(_type: string) {
	try {
		await cancelFlash();
	} catch (error) {
		console.error('Error cancelling flash:', error);
	}
}
