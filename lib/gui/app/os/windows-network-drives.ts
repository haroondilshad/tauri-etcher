/*
 * Copyright 2019 balena.io
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

/**
 * @summary Replaces network drive letter with network drive location in the provided filePath on Windows
 * 
 * Note: In Tauri, this functionality would need to be implemented in the Rust backend
 * or the sidecar if needed. For now, we just return the original path.
 */
export async function replaceWindowsNetworkDriveLetter(
	filePath: string,
): Promise<string> {
	// In Tauri's webview, we can't directly access Windows APIs like wmic
	// The sidecar handles file operations and would need to resolve network paths
	return filePath;
}
