/** Format a byte count as a human-readable string (e.g., "1.2 MB"). */
export function formatBytes(bytes: number): string {
	if (bytes === 0) return '0 B';
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	const i = Math.floor(Math.log(bytes) / Math.log(1024));
	const value = bytes / Math.pow(1024, i);
	return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`;
}

/** Format an ISO date string as a relative time (e.g., "3 hours ago"). */
export function relativeTime(iso: string): string {
	const now = Date.now();
	const then = new Date(iso).getTime();
	const seconds = Math.floor((now - then) / 1000);

	if (seconds < 60) return 'just now';
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'} ago`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours} hour${hours === 1 ? '' : 's'} ago`;
	const days = Math.floor(hours / 24);
	if (days < 30) return `${days} day${days === 1 ? '' : 's'} ago`;
	const months = Math.floor(days / 30);
	if (months < 12) return `${months} month${months === 1 ? '' : 's'} ago`;
	const years = Math.floor(months / 12);
	return `${years} year${years === 1 ? '' : 's'} ago`;
}

/** Truncate a hash to a short display form (first 8 chars). */
export function shortHash(hash: string): string {
	return hash.slice(0, 8);
}
