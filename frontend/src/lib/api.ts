import { auth } from './auth.svelte';
import type { AuthResponse, DeviceCodeResponse, ProjectInfo, UserInfo } from './types';

const API_BASE = import.meta.env.PUBLIC_API_BASE || 'https://api.ozzydb.com';

class ApiError extends Error {
	status: number;
	code: string;
	constructor(status: number, code: string, message: string) {
		super(message);
		this.status = status;
		this.code = code;
	}
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
	const headers = new Headers(options.headers);
	if (!headers.has('Content-Type') && options.body) {
		headers.set('Content-Type', 'application/json');
	}
	if (auth.token) {
		headers.set('Authorization', `Bearer ${auth.token}`);
	}

	const res = await fetch(`${API_BASE}/api/v1${path}`, { ...options, headers });
	if (!res.ok) {
		const body = await res.json().catch(() => ({ error: 'unknown', message: res.statusText }));
		throw new ApiError(res.status, body.error, body.message);
	}
	if (res.status === 204 || res.headers.get('content-length') === '0') {
		return undefined as T;
	}
	return res.json();
}

// Auth
export function initiateDeviceFlow() {
	return request<DeviceCodeResponse>('/auth/github/device', { method: 'POST' });
}

export function pollDeviceFlow(deviceCode: string) {
	return request<AuthResponse>('/auth/github/poll', {
		method: 'POST',
		body: JSON.stringify({ device_code: deviceCode, client: 'web' })
	});
}

export function getMe() {
	return request<UserInfo>('/auth/me');
}

// Projects (v2: will be reimplemented with new API)
export function listProjects(limit = 50, offset = 0) {
	return request<ProjectInfo[]>(`/projects?limit=${limit}&offset=${offset}`);
}

export function getProject(owner: string, project: string) {
	return request<ProjectInfo>(`/${owner}/${project}`);
}

export { ApiError };
