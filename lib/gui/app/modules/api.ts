/** This function will :
 * 	- start the ipc server (api)
 *  - spawn the child process (privileged or not)
 *  - wait for the child process to connect to the api
 *  - return a promise that will resolve with the emit function for the api
 *
 * //TODO:
 *  - this should be refactored to reverse the control flow:
 *    - the child process should be the server
 *    - this should be the client
 *  - replace the current node-ipc api with a websocket api
 *  - centralise the api for both the writer and the scanner instead of having two instances running
 */

import { invoke } from '@tauri-apps/api/core';
import { Command } from '@tauri-apps/plugin-shell';
import * as packageJSON from '../../../../package.json';
import * as permissions from '../../../shared/permissions';
import * as errors from '../../../shared/errors';

const THREADS_PER_CPU = 16;
const connectionRetryDelay = 1000;
const connectionRetryAttempts = 10;

// Use Node.js directly instead of pkg-bundled binary (to avoid ext2fs WASM loading issues)
const USE_NODE_DIRECTLY = true;

// Check if we're running in a Tauri environment
function isTauriEnvironment(): boolean {
	return typeof window !== 'undefined' && '__TAURI__' in window;
}

// Get the number of CPUs (fallback for browser environment)
function getCpuCount(): number {
	return navigator.hardwareConcurrency || 4;
}

// Get the sidecar path from Tauri backend
async function getSidecarPath(): Promise<string> {
	if (!isTauriEnvironment()) {
		throw new Error('Not running in Tauri environment');
	}
	return await invoke<string>('get_sidecar_path');
}

async function writerArgv(): Promise<string[]> {
	const entryPoint = await getSidecarPath();
	return [entryPoint];
}

async function spawnChild(
	withPrivileges: boolean,
	etcherServerId: string,
	etcherServerAddress: string,
	etcherServerPort: string,
) {
	// Check if we're in Tauri environment
	if (!isTauriEnvironment()) {
		throw new Error('Sidecar spawning requires Tauri environment. Open this app in the Tauri window, not a browser.');
	}

	const env: Record<string, string> = {
		ETCHER_SERVER_ADDRESS: etcherServerAddress,
		ETCHER_SERVER_ID: etcherServerId,
		ETCHER_SERVER_PORT: etcherServerPort,
		UV_THREADPOOL_SIZE: (getCpuCount() * THREADS_PER_CPU).toString(),
		// This environment variable prevents the AppImages
		// desktop integration script from presenting the
		// "installation" dialog
		SKIP: '1',
	};

	if (USE_NODE_DIRECTLY) {
		// Use Node.js to run the sidecar script directly (bypasses pkg WASM issues)
		const sidecarScriptPath = await invoke<string>('get_sidecar_script_path');
		console.log('Using Node.js to run sidecar script:', sidecarScriptPath);
		
		if (withPrivileges) {
			console.log('... with privileges ...');
			// For privileged, we need to run node with sudo
			return permissions.elevateCommand(['node', sidecarScriptPath], {
				applicationName: packageJSON.displayName,
				env,
			});
		} else {
			// Spawn node directly with the script
			const command = Command.create('node', [sidecarScriptPath], { env });
			
			command.stdout.on('data', (data: string) => {
				console.log('Sidecar stdout:', data);
			});
			command.stderr.on('data', (data: string) => {
				console.log('Sidecar stderr:', data);
			});
			command.on('close', (data: { code: number }) => {
				console.log('Sidecar exited with code:', data.code);
			});
			
			const child = await command.spawn();
			console.log('Spawned unprivileged sidecar (via Node.js) with PID:', child.pid);
			
			return { cancelled: false, spawned: child };
		}
	} else {
		// Original path: use pkg-bundled binary
		const argv = await writerArgv();
		
		if (withPrivileges) {
			console.log('... with privileges ...');
			return permissions.elevateCommand(argv, {
				applicationName: packageJSON.displayName,
				env,
			});
		} else {
			// Use Tauri's shell plugin to spawn the sidecar
			// The sidecar name must match the path in tauri.conf.json's externalBin
			const command = Command.sidecar('binaries/etcher-util', [], {
				env,
			});

			// For unprivileged spawn, we use a direct spawn approach
			// The sidecar will start its own WebSocket server
			const child = await command.spawn();
			console.log('Spawned unprivileged sidecar with PID:', child.pid);
			
			return { cancelled: false, spawned: child };
		}
	}
}

type ChildApi = {
	emit: (type: string, payload: any) => void;
	registerHandler: (event: string, handler: any) => void;
	failed: boolean;
};

async function connectToChildProcess(
	etcherServerAddress: string,
	etcherServerPort: string,
	etcherServerId: string,
): Promise<ChildApi | { failed: boolean }> {
	return new Promise((resolve, reject) => {
		console.log(etcherServerId);

		const url = `ws://${etcherServerAddress}:${etcherServerPort}`;

		// Use browser WebSocket API
		const ws = new WebSocket(url);

		let heartbeat: ReturnType<typeof setInterval>;

		const startHeartbeat = (emit: (type: string, payload: any) => void) => {
			console.log('start heartbeat');
			heartbeat = setInterval(() => {
				emit('heartbeat', {});
			}, 1000);
		};

		const stopHeartbeat = () => {
			console.log('stop heartbeat');
			clearInterval(heartbeat);
		};

		ws.onerror = (error: Event) => {
			console.log('WebSocket error:', error);
			resolve({
				failed: true,
			});
		};

		ws.onopen = () => {
			const emit = (type: string, payload: any) => {
				ws.send(JSON.stringify({ type, payload }));
			};

			emit('ready', {});

			// parse and route messages
			const messagesHandler: Record<string, (payload: any) => void> = {
				log: (message: any) => {
					console.log(`CHILD LOG: ${message}`);
				},

				error: (error: any) => {
					const errorObject = errors.fromJSON(error);
					console.error('CHILD ERROR', errorObject);
					stopHeartbeat();
				},

				// once api is ready (means child process is connected) we pass the emit function to the caller
				ready: () => {
					console.log('CHILD READY');

					startHeartbeat(emit);

					resolve({
						failed: false,
						emit,
						registerHandler,
					});
				},
			};

			ws.onmessage = (event: MessageEvent) => {
				const data = JSON.parse(event.data);
				const message = messagesHandler[data.type];
				if (message) {
					message(data.payload);
				} else {
					console.warn(`Unknown message type: ${data.type}`);
				}
			};

			// api to register more handlers with callbacks
			const registerHandler = (event: string, handler: (payload: any) => void) => {
				messagesHandler[event] = handler;
			};
		};
	});
}

async function spawnChildAndConnect({
	withPrivileges,
}: {
	withPrivileges: boolean;
}): Promise<ChildApi> {
	const etcherServerAddress = '127.0.0.1'; // localhost
	const etcherServerPort = withPrivileges ? '3435' : '3434';
	const etcherServerId = `etcher-${Math.random().toString(36).substring(7)}`;

	console.log(
		`Starting ${
			withPrivileges ? 'priviledged' : 'unpriviledged'
		} flasher sidecar on port ${etcherServerPort}`,
	);

	// spawn the child process, which will act as the ws server
	try {
		const result = await spawnChild(
			withPrivileges,
			etcherServerId,
			etcherServerAddress,
			etcherServerPort,
		);
		if (result.cancelled) {
			throw new Error('Starting flasher sidecar process was cancelled');
		}
	} catch (error) {
		console.error('Error starting flasher sidecar process', error);
		throw new Error('Error starting flasher sidecar process');
	}

	// try to connect to the ws server, retrying if necessary, until the connection is established
	try {
		let retry = 0;
		while (retry < connectionRetryAttempts) {
			const { emit, registerHandler, failed } = await connectToChildProcess(
				etcherServerAddress,
				etcherServerPort,
				etcherServerId,
			);
			if (failed) {
				retry++;
				console.log(
					`Connection to sidecar flasher process attempt ${retry} / ${connectionRetryAttempts} failed; retrying in ${connectionRetryDelay}ms...`,
				);
				await new Promise((resolve) =>
					setTimeout(resolve, connectionRetryDelay),
				);
				continue;
			}
			return { failed, emit, registerHandler };
		}
		throw new Error('Connection to sidecar flasher process timed out');
	} catch (error) {
		console.error('Error connecting to sidecar flasher process process', error);
		throw new Error('Connection to sidecar flasher process failed');
	}
}

export { spawnChildAndConnect };
