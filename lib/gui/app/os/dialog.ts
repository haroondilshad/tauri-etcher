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

import { open as tauriOpen, message as tauriMessage, ask as tauriAsk } from '@tauri-apps/plugin-dialog';
import * as _ from 'lodash';

import * as errors from '../../../shared/errors';
import { SUPPORTED_EXTENSIONS } from '../../../shared/supported-formats';
import * as i18next from 'i18next';

/**
 * @summary Open an image selection dialog
 *
 * @description
 * Notice that by image, we mean *.img/*.iso/*.zip/etc files.
 */
export async function selectImage(): Promise<string | undefined> {
	const result = await tauriOpen({
		multiple: false,
		directory: false,
		filters: [
			{
				name: i18next.t('source.osImages'),
				extensions: SUPPORTED_EXTENSIONS,
			},
			{
				name: i18next.t('source.allFiles'),
				extensions: ['*'],
			},
		],
	});

	// tauriOpen returns null if cancelled, or a string/array for selected file(s)
	if (result === null) {
		return undefined;
	}

	// If multiple is false, result is a single path string
	return typeof result === 'string' ? result : result?.[0];
}

/**
 * @summary Open a warning dialog
 */
export async function showWarning(options: {
	confirmationLabel: string;
	rejectionLabel: string;
	title: string;
	description: string;
}): Promise<boolean> {
	_.defaults(options, {
		confirmationLabel: i18next.t('ok'),
		rejectionLabel: i18next.t('cancel'),
	});

	// Tauri's ask dialog returns true for "Yes" (first button) and false for "No"
	const confirmed = await tauriAsk(options.description, {
		title: options.title,
		kind: 'warning',
		okLabel: options.confirmationLabel,
		cancelLabel: options.rejectionLabel,
	});

	return confirmed;
}

/**
 * @summary Show error dialog for an Error instance
 */
export async function showError(error: Error) {
	const title = errors.getTitle(error);
	const description = errors.getDescription(error);
	
	await tauriMessage(description, {
		title: title,
		kind: 'error',
	});
}
