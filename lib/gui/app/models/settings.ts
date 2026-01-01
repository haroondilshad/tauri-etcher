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

import * as _ from 'lodash';

// Browser-compatible debug (debug package uses process.env which is not available)
const debug = (...args: any[]) => {
	if (typeof console !== 'undefined' && console.log) {
		console.log('[etcher:models:settings]', ...args);
	}
};

export const DEFAULT_WIDTH = 800;
export const DEFAULT_HEIGHT = 480;

// Use localStorage for settings in Tauri (browser environment)
const STORAGE_KEY = 'etcher-settings';

function readConfigFromStorage(): _.Dictionary<any> {
	try {
		const stored = localStorage.getItem(STORAGE_KEY);
		if (stored) {
			return JSON.parse(stored);
		}
	} catch (error: any) {
		console.error('Error reading settings:', error);
	}
	return {};
}

function writeConfigToStorage(data: _.Dictionary<any>): void {
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
	} catch (error: any) {
		console.error('Error writing settings:', error);
		throw error;
	}
}

// exported for tests
export async function readAll() {
	return readConfigFromStorage();
}

// exported for tests
export async function writeConfigFile(
	_filename: string,
	data: _.Dictionary<any>,
): Promise<void> {
	writeConfigToStorage(data);
}

const DEFAULT_SETTINGS: _.Dictionary<any> = {
	errorReporting: true,
	updatesEnabled: false, // Tauri has its own updater
	desktopNotifications: true,
	autoBlockmapping: true,
	decompressFirst: true,
};

const settings = _.cloneDeep(DEFAULT_SETTINGS);

async function load(): Promise<void> {
	debug('load');
	const loadedSettings = await readAll();
	_.assign(settings, loadedSettings);
}

const loaded = load();

export async function set(
	key: string,
	value: any,
	writeConfigFileFn = writeConfigFile,
): Promise<void> {
	debug('set', key, value);
	await loaded;
	const previousValue = settings[key];
	settings[key] = value;
	try {
		await writeConfigFileFn('', settings);
	} catch (error: any) {
		// Revert to previous value if persisting settings failed
		settings[key] = previousValue;
		throw error;
	}
}

export async function get(key: string): Promise<any> {
	await loaded;
	return getSync(key);
}

export function getSync(key: string): any {
	return _.cloneDeep(settings[key]);
}

export async function getAll() {
	debug('getAll');
	await loaded;
	return _.cloneDeep(settings);
}
