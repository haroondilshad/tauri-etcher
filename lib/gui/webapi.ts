//
// Anything exported from this module will become available to the
// frontend. They're accessible as `window.etcher.foo()` for backwards
// compatibility, but with Tauri we prefer using invoke directly.
//

import { invoke } from '@tauri-apps/api/core';

export async function getEtcherUtilPath(): Promise<string> {
	const utilPath = await invoke<string>('get_sidecar_path');
	console.log('Sidecar path:', utilPath);
	return utilPath;
}
