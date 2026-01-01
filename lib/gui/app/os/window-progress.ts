/*
 * Copyright 2016 balena.io
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

import type { FlashState } from '../modules/progress-status';
import { titleFromFlashState } from '../modules/progress-status';

/**
 * @summary The title of the main window upon program launch
 */
const INITIAL_TITLE = 'balenaEtcher';

/**
 * @summary Make the full window status title
 */
function getWindowTitle(state?: FlashState) {
	if (state) {
		return `${INITIAL_TITLE} – ${titleFromFlashState(state)}`;
	}
	return INITIAL_TITLE;
}

/**
 * @summary Set operating system window progress
 *
 * @description
 * Show progress inline in operating system task bar.
 * Note: Tauri doesn't have a direct setProgressBar API like Electron,
 * but we can still update the window title to show progress.
 */
export function set(state: FlashState) {
	// Update document title to show progress
	document.title = getWindowTitle(state);
	
	// Note: For taskbar progress in Tauri, we would need to use
	// platform-specific Rust code or a Tauri plugin.
	// For now, we just update the window title.
}

/**
 * @summary Clear the window progress bar
 */
export function clear() {
	document.title = getWindowTitle(undefined);
}
