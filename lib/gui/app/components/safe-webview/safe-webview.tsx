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

import * as React from 'react';

interface SafeWebviewProps {
	// The website source URL
	src: string;
	// Webview lifecycle event
	onWebviewShow?: (isWebviewShowing: boolean) => void;
	style?: React.CSSProperties;
}

/**
 * @summary Stub component for SafeWebview
 * 
 * In the Electron version, this component displayed a webview with
 * featured projects during flashing. Since Tauri doesn't support
 * the <webview> tag, we simply don't render this content.
 * 
 * The onWebviewShow callback is called with false to indicate
 * the webview is not showing, which adjusts the layout accordingly.
 */
export class SafeWebview extends React.PureComponent<SafeWebviewProps> {
	public componentDidMount() {
		// Notify parent that webview is not showing
		if (this.props.onWebviewShow) {
			this.props.onWebviewShow(false);
		}
	}

	public render() {
		// Don't render anything - webview not supported in Tauri
		return null;
	}
}
