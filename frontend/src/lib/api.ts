import { auth } from './auth.svelte';
import type {
	AuthResponse,
	CollaboratorInfo,
	CommitInfo,
	DagResponse,
	DeviceCodeResponse,
	EndpointsAtRefResponse,
	ListRefsResponse,
	ProjectInfo,
	TransformsAtRefResponse,
	UserInfo
} from './types';

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

async function requestRaw(path: string, options: RequestInit = {}): Promise<Response> {
	const headers = new Headers(options.headers);
	if (auth.token) {
		headers.set('Authorization', `Bearer ${auth.token}`);
	}
	return fetch(`${API_BASE}/api/v1${path}`, { ...options, headers });
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

// Projects
export function listProjects(limit = 50, offset = 0) {
	return request<ProjectInfo[]>(`/projects?limit=${limit}&offset=${offset}`);
}

export function getProject(owner: string, project: string) {
	return request<ProjectInfo>(`/${owner}/${project}`);
}

// Commits
export function listCommits(owner: string, project: string, limit = 50, offset = 0) {
	return request<CommitInfo[]>(
		`/${owner}/${project}/commits?limit=${limit}&offset=${offset}`
	);
}

// DAG
export function getDag(owner: string, project: string, ref?: string) {
	const q = ref ? `?ref=${encodeURIComponent(ref)}` : '';
	return request<DagResponse>(`/${owner}/${project}/dag${q}`);
}

export async function getDagSvg(owner: string, project: string, ref?: string) {
	const q = ref ? `?ref=${encodeURIComponent(ref)}` : '';
	const res = await requestRaw(`/${owner}/${project}/dag.svg${q}`);
	if (!res.ok) throw new ApiError(res.status, 'svg_error', 'Failed to load DAG SVG');
	return res.text();
}

// Endpoints & Transforms
export function listEndpoints(owner: string, project: string, ref?: string) {
	const q = ref ? `?ref=${encodeURIComponent(ref)}` : '';
	return request<EndpointsAtRefResponse>(`/${owner}/${project}/endpoints${q}`);
}

export function listTransforms(owner: string, project: string, ref?: string) {
	const q = ref ? `?ref=${encodeURIComponent(ref)}` : '';
	return request<TransformsAtRefResponse>(`/${owner}/${project}/transforms${q}`);
}

// Refs
export function listRefs(owner: string, project: string) {
	return request<ListRefsResponse>(`/${owner}/${project}/refs`);
}

// Collaborators
export function listCollaborators(owner: string, project: string) {
	return request<CollaboratorInfo[]>(`/${owner}/${project}/collaborators`);
}

export { ApiError };
