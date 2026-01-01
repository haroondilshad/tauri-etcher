/*
 * Browser-compatible path utilities
 * These provide path manipulation functions that work in both Node.js and browser environments
 */

/**
 * Get the base name of a path (the last portion)
 */
export function basename(filePath: string, ext?: string): string {
	if (!filePath) return '';
	// Handle both Windows and Unix paths
	const parts = filePath.replace(/\\/g, '/').split('/');
	let name = parts[parts.length - 1] || parts[parts.length - 2] || '';
	
	// Remove extension if specified
	if (ext && name.endsWith(ext)) {
		name = name.slice(0, -ext.length);
	}
	
	return name;
}

/**
 * Get the directory name of a path
 */
export function dirname(filePath: string): string {
	if (!filePath) return '.';
	// Handle both Windows and Unix paths
	const normalized = filePath.replace(/\\/g, '/');
	const parts = normalized.split('/');
	parts.pop();
	return parts.join('/') || '/';
}

/**
 * Get the extension of a path
 */
export function extname(filePath: string): string {
	const base = basename(filePath);
	const dotIndex = base.lastIndexOf('.');
	if (dotIndex === -1 || dotIndex === 0) return '';
	return base.slice(dotIndex);
}

/**
 * Join path segments
 */
export function join(...paths: string[]): string {
	const parts: string[] = [];
	
	for (const path of paths) {
		if (!path) continue;
		const normalized = path.replace(/\\/g, '/');
		const segments = normalized.split('/').filter(Boolean);
		parts.push(...segments);
	}
	
	return parts.join('/');
}

/**
 * Normalize a path
 */
export function normalize(filePath: string): string {
	if (!filePath) return '.';
	const normalized = filePath.replace(/\\/g, '/');
	const parts = normalized.split('/');
	const result: string[] = [];
	
	for (const part of parts) {
		if (part === '..') {
			result.pop();
		} else if (part !== '.' && part !== '') {
			result.push(part);
		}
	}
	
	return (normalized.startsWith('/') ? '/' : '') + result.join('/');
}

/**
 * Check if a path is absolute
 */
export function isAbsolute(filePath: string): boolean {
	if (!filePath) return false;
	// Unix absolute path
	if (filePath.startsWith('/')) return true;
	// Windows absolute path (e.g., C:\)
	if (/^[A-Za-z]:[/\\]/.test(filePath)) return true;
	return false;
}

/**
 * Check if a path is inside another path (potential parent directory)
 * Browser-compatible replacement for the 'path-is-inside' npm package
 */
export function pathIsInside(thePath: string, potentialParent: string): boolean {
	if (!thePath || !potentialParent) return false;
	
	// Normalize paths - convert backslashes to forward slashes
	let normalizedPath = thePath.replace(/\\/g, '/');
	let normalizedParent = potentialParent.replace(/\\/g, '/');
	
	// Strip trailing slashes
	if (normalizedPath.endsWith('/')) {
		normalizedPath = normalizedPath.slice(0, -1);
	}
	if (normalizedParent.endsWith('/')) {
		normalizedParent = normalizedParent.slice(0, -1);
	}
	
	// Check if thePath starts with potentialParent
	// and the next character (if any) is a path separator or end of string
	if (normalizedPath.indexOf(normalizedParent) !== 0) {
		return false;
	}
	
	const nextChar = normalizedPath[normalizedParent.length];
	return nextChar === '/' || nextChar === undefined;
}

// For compatibility, export a default object with all functions
export default {
	basename,
	dirname,
	extname,
	join,
	normalize,
	isAbsolute,
	pathIsInside,
};
