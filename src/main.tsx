/*
 * Copyright 2016 balena.io
 * Copyright 2024 - Tauri Migration
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

import type { Dictionary } from 'lodash';
import { debounce, capitalize, values } from 'lodash';
import outdent from 'outdent';
import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { v4 as uuidV4 } from 'uuid';

import * as packageJSON from '../package.json';
import type { DrivelistDrive } from '../lib/shared/drive-constraints';
import * as messages from '../lib/shared/messages';
import * as availableDrives from '../lib/gui/app/models/available-drives';
import * as flashState from '../lib/gui/app/models/flash-state';
import * as settings from '../lib/gui/app/models/settings';
import { Actions, observe, store } from '../lib/gui/app/models/store';
import * as analytics from '../lib/gui/app/modules/analytics';
import { startDrivePolling, getSourceMetadata, toSourceMetadata } from '../lib/gui/app/modules/api-rust';
import * as exceptionReporter from '../lib/gui/app/modules/exception-reporter';
import * as osDialog from '../lib/gui/app/os/dialog';
import MainPage from '../lib/gui/app/pages/main/MainPage';
import { setRequestMetadata } from '../lib/gui/app/components/source-selector/source-selector';
import '../lib/gui/app/css/main.css';
import '../lib/gui/app/i18n';
import { langParser } from '../lib/gui/app/i18n';
import * as i18next from 'i18next';
import type { SourceMetadata } from '../lib/shared/typings/source-selector';

// Initialize i18n
i18next.changeLanguage(langParser());

// Global error handler
window.addEventListener(
	'unhandledrejection',
	(event: PromiseRejectionEvent | any) => {
		const error = event.reason || event;
		analytics.logException(error);
		event.preventDefault();
	},
);

// Set application session UUID
store.dispatch({
	type: Actions.SET_APPLICATION_SESSION_UUID,
	data: uuidV4(),
});

// Set first flashing workflow UUID
store.dispatch({
	type: Actions.SET_FLASHING_WORKFLOW_UUID,
	data: uuidV4(),
});

console.log(outdent`
	${outdent}
	 _____ _       _
	|  ___| |     | |
	| |__ | |_ ___| |__   ___ _ __
	|  __|| __/ __| '_ \\ / _ \\ '__|
	| |___| || (__| | | |  __/ |
	\\____/ \\__\\___|_| |_|\\___|_|

	Interested in joining the Etcher team?
	Drop us a line at join+etcher@balena.io

	Version = ${packageJSON.version} (Tauri)
`);

const debouncedLog = debounce(console.log, 1000, { maxWait: 1000 });

function pluralize(word: string, quantity: number) {
	return `${quantity} ${word}${quantity === 1 ? '' : 's'}`;
}

// Observe flash state changes and log progress
observe(() => {
	if (!flashState.isFlashing()) {
		return;
	}
	const currentFlashState = flashState.getFlashState();

	let eta = '';
	if (currentFlashState.eta !== undefined) {
		eta = `eta in ${currentFlashState.eta.toFixed(0)}s`;
	}
	let active = '';
	if (currentFlashState.type !== 'decompressing') {
		active = pluralize('device', currentFlashState.active);
	}
	debouncedLog(outdent({ newline: ' ' })`
		${capitalize(currentFlashState.type)}
		${active},
		${currentFlashState.percentage}%
		at
		${(currentFlashState.speed || 0).toFixed(2)}
		MB/s
		(total ${(currentFlashState.speed * currentFlashState.active).toFixed(2)} MB/s)
		${eta}
		with
		${pluralize('failed device', currentFlashState.failed)}
	`);
});

function setDrives(drives: DrivelistDrive[]) {
	// Prevent setting drives while flashing to avoid losing some while unmounting
	if (!flashState.isFlashing()) {
		availableDrives.setDrives(drives);
	}
}

// Start drive polling using native Rust API (no sidecar needed!)
const stopDrivePolling = startDrivePolling(setDrives, 2000);

// Set up requestMetadata function using native Rust API
const requestMetadataFn = async (params: { selected: string; SourceType: string; auth?: any }): Promise<SourceMetadata> => {
	try {
		const rustMeta = await getSourceMetadata(params.selected);
		return {
			...toSourceMetadata(rustMeta, params.selected),
			SourceType: params.SourceType as any,
		} as SourceMetadata;
	} catch (error) {
		console.error('Error getting source metadata:', error);
		return {} as SourceMetadata;
	}
};

// Set the requestMetadata function for use in source-selector
setRequestMetadata(requestMetadataFn);

let popupExists = false;

// Initialize analytics
analytics.initAnalytics();

// Handle window close during flashing
window.addEventListener('beforeunload', async (event) => {
	if (!flashState.isFlashing() || popupExists) {
		return;
	}

	// Don't close window while flashing
	event.returnValue = false;
	popupExists = true;

	try {
		const confirmed = await osDialog.showWarning({
			confirmationLabel: i18next.t('yesExit'),
			rejectionLabel: i18next.t('cancel'),
			title: i18next.t('reallyExit'),
			description: messages.warning.exitWhileFlashing(),
		});
		if (confirmed) {
			// Use Tauri process API to exit
			const { exit } = await import('@tauri-apps/plugin-process');
			await exit(0);
		}
		popupExists = false;
	} catch (error: any) {
		exceptionReporter.report(error);
	}
});

// Main render function
async function main() {
	// Initialize LEDs (for Etcher Pro hardware)
	try {
		const { init: ledsInit } = await import('../lib/gui/app/models/leds');
		await ledsInit();
	} catch (error: any) {
		// LEDs not available on most systems, ignore
	}

	// Render the React app
	const container = document.getElementById('main');
	if (container) {
		const root = createRoot(container);
		root.render(React.createElement(MainPage));
	}
}

// Start the application
main();
