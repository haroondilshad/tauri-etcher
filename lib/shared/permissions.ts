/*
 * Copyright 2017 balena.io
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

import * as _ from 'lodash';
import { invoke } from '@tauri-apps/api/core';

/**
 * @summary Check if the current process is running with elevated permissions
 * 
 * In Tauri, we can't directly check this from the webview.
 * The sidecar handles privilege elevation.
 */
export async function isElevated(): Promise<boolean> {
	// In Tauri, we delegate privilege checking to the Rust backend or sidecar
	// For now, return false as the sidecar handles elevation
	return false;
}

/**
 * @summary Check if the current process is running with elevated permissions (sync)
 */
export function isElevatedUnixSync(): boolean {
	// Can't check from browser context
	return false;
}

/**
 * @summary Elevate and execute a command
 * 
 * In Tauri, privilege elevation for the sidecar is handled differently.
 * This function will spawn the sidecar with privilege elevation using
 * Tauri's shell plugin which calls platform-specific sudo mechanisms.
 */
export async function elevateCommand(
	command: string[],
	options: {
		env: Record<string, string | undefined>;
		applicationName: string;
	},
): Promise<{ cancelled: boolean; spawned?: any }> {
	// Import Tauri shell dynamically to handle cases where it might not be available
	try {
		const { Command } = await import('@tauri-apps/plugin-shell');
		
		// Get platform to determine elevation strategy
		const platform = navigator.platform.toLowerCase();
		
		// Build environment variables
		const env: Record<string, string> = {};
		for (const [key, value] of Object.entries(options.env)) {
			if (value !== undefined) {
				env[key] = value;
			}
		}

		if (platform.includes('win')) {
			// On Windows, use PowerShell with Start-Process -Verb RunAs
			const args = command.slice(1).map(arg => `"${arg}"`).join(' ');
			const psCommand = `Start-Process -FilePath "${command[0]}" -ArgumentList '${args}' -Verb RunAs -Wait`;
			
			const shellCmd = Command.create('powershell', ['-Command', psCommand], { env });
			const child = await shellCmd.spawn();
			
			return { cancelled: false, spawned: child };
		} else 		if (platform.includes('mac')) {
			// On macOS, using osascript's "with administrator privileges" runs commands
			// in a restricted root context where TCC blocks access to ~/Downloads.
			// 
			// Solution: Use `sudo -E --askpass` with a custom askpass script.
			// - `-E` preserves the user's environment (HOME, PATH, file access)
			// - `--askpass` uses a script to prompt for password via osascript dialog
			// This runs the command as root while preserving file access permissions.
			
			const envFilter = [
				'ETCHER_SERVER_ADDRESS',
				'ETCHER_SERVER_PORT',
				'ETCHER_SERVER_ID',
				'ETCHER_NO_SPAWN_UTIL',
				'ETCHER_TERMINATE_TIMEOUT',
				'UV_THREADPOOL_SIZE',
				'SKIP',
			];
			
			// Build command with env as args: --KEY=value
			const envArgs = Object.entries(env)
				.filter(([key]) => envFilter.includes(key))
				.map(([key, value]) => `--${key}=${value}`);
			
			// Handle 'node' command specially - use the SAME node that compiled the native modules
			// The native modules (mountutils, drivelist) were compiled against a specific Node.js version
			// We MUST use that same version or we'll get NODE_MODULE_VERSION mismatch errors
			let cmdParts: string[];
			if (command[0] === 'node' && command.length > 1) {
				// Use absolute path to the NVM-managed Node.js that compiled the native modules
				const nodePath = '/Users/callmenuwanda/.nvm/versions/node/v20.19.0/bin/node';
				cmdParts = [nodePath, command[1]];
			} else {
				cmdParts = [command[0]];
			}
			
			// Build the command string with proper escaping for sh -c
			const escapedCmdParts = cmdParts.map(part => `'${part.replace(/'/g, "'\"'\"'")}'`).join(' ');
			const argsStr = envArgs.map(arg => `'${arg.replace(/'/g, "'\"'\"'")}'`).join(' ');
			
			// Get the askpass script path - it's relative to this module
			// In dev, it's in the source tree; in production, it would be bundled
			const askpassPath = '/Users/callmenuwanda/Projects/my-new-appsa/tauri-etcher/lib/shared/sudo-askpass.sh';
			
			// Build the full command to run in background
			// Run directly without backgrounding first - let's see the actual output/errors
			const innerCommand = `${escapedCmdParts} ${argsStr}`;
			
			console.log('Elevating with sudo -E --askpass');
			console.log('Command:', innerCommand);
			
			// Use sudo -E --askpass to run the command
			// The askpass script will show a password dialog
			// The -E flag preserves environment variables including those needed for disk access
			const shellCmd = Command.create('sudo', ['-E', '--askpass', 'sh', '-c', innerCommand], {
				env: {
					...env,
					SUDO_ASKPASS: askpassPath,
					// Ensure PATH includes common locations for binaries
					PATH: '/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin',
				},
			});
			
			// Listen for stdout/stderr to debug sidecar issues
			shellCmd.on('close', (data: { code: number }) => {
				console.log('osascript process exited with code:', data.code);
			});
			shellCmd.stdout.on('data', (data: string) => {
				console.log('osascript stdout (PID):', data.trim());
			});
			shellCmd.stderr.on('data', (data: string) => {
				console.log('osascript stderr:', data);
			});
			
			const child = await shellCmd.spawn();
			
			return { cancelled: false, spawned: child };
		} else {
			// On Linux, try pkexec or sudo
			// Try pkexec first (works with most desktop environments)
			try {
				const shellCmd = Command.create('pkexec', ['env', ...Object.entries(env).map(([k, v]) => `${k}=${v}`), ...command]);
				const child = await shellCmd.spawn();
				return { cancelled: false, spawned: child };
			} catch {
				// Fall back to sudo with terminal
				const shellCmd = Command.create('sudo', command, { env });
				const child = await shellCmd.spawn();
				return { cancelled: false, spawned: child };
			}
		}
	} catch (error: any) {
		console.error('Failed to elevate command:', error);
		// Check if user cancelled
		if (error.message?.includes('cancelled') || error.message?.includes('User did not grant permission')) {
			return { cancelled: true };
		}
		throw error;
	}
}
