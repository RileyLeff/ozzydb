import { auth } from './auth.svelte';
import type {
	AuthResponse,
	CollectionDetail,
	CollectionInfo,
	CommitDetail,
	CommitSummary,
	DagResponse,
	DataAtom,
	DataAtomDetail,
	DeviceCodeResponse,
	EndpointDetail,
	EndpointSummary,
	FlattenedAtom,
	MetadataEntry,
	ProjectDetail,
	ProjectInfo,
	SecretInfo,
	UserInfo,
	VersionLogEntry
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
	if (!headers.has('Content-Type') && options.body && !(options.body instanceof FormData)) {
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

/** Make a raw fetch (for binary responses like downloads). */
async function rawRequest(path: string, options: RequestInit = {}): Promise<Response> {
	const headers = new Headers(options.headers);
	if (auth.token) {
		headers.set('Authorization', `Bearer ${auth.token}`);
	}
	const res = await fetch(`${API_BASE}/api/v1${path}`, { ...options, headers });
	if (!res.ok) {
		const body = await res.json().catch(() => ({ error: 'unknown', message: res.statusText }));
		throw new ApiError(res.status, body.error, body.message);
	}
	return res;
}

// ── Auth ──────────────────────────────────────────────────────

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

// ── Projects ─────────────────────────────────────────────────

export function listProjects(owner: string) {
	return request<ProjectInfo[]>(`/projects/${owner}`);
}

export function getProject(owner: string, slug: string) {
	return request<ProjectDetail>(`/projects/${owner}/${slug}`);
}

// ── Data Atoms ───────────────────────────────────────────────

export function listData(owner: string, project: string) {
	return request<DataAtom[]>(`/data/${owner}/${project}`);
}

export function getDataAtom(owner: string, project: string, name: string) {
	return request<DataAtomDetail>(`/data/${owner}/${project}/${name}`);
}

export function getMetadata(owner: string, project: string, name: string) {
	return request<MetadataEntry[]>(`/data/${owner}/${project}/${name}/metadata`);
}

export function downloadData(owner: string, project: string, name: string) {
	return rawRequest(`/data/${owner}/${project}/${name}/download`);
}

export function yankData(owner: string, project: string, name: string, reason: string) {
	return request<void>(`/data/${owner}/${project}/${name}/yank`, {
		method: 'POST',
		body: JSON.stringify({ reason })
	});
}

export function describeData(
	owner: string,
	project: string,
	name: string,
	field: string,
	value: unknown
) {
	return request<MetadataEntry>(`/data/${owner}/${project}/${name}/describe`, {
		method: 'POST',
		body: JSON.stringify({ field, value })
	});
}

// ── Collections ──────────────────────────────────────────────

export function listCollections(owner: string, project: string) {
	return request<CollectionInfo[]>(`/collections/${owner}/${project}`);
}

export function getCollection(owner: string, project: string, name: string) {
	return request<CollectionDetail>(`/collections/${owner}/${project}/${name}`);
}

export function getCollectionLog(owner: string, project: string, name: string) {
	return request<VersionLogEntry[]>(`/collections/${owner}/${project}/${name}/log`);
}

export function flattenCollection(owner: string, project: string, name: string) {
	return request<FlattenedAtom[]>(`/collections/${owner}/${project}/${name}/flatten`);
}

// ── Endpoints ────────────────────────────────────────────────

export function listEndpoints(owner: string, project: string, ref?: string) {
	const params = ref ? `?ref=${encodeURIComponent(ref)}` : '';
	return request<EndpointSummary[]>(`/endpoints/${owner}/${project}${params}`);
}

export function getEndpoint(owner: string, project: string, name: string, ref?: string) {
	const params = ref ? `?ref=${encodeURIComponent(ref)}` : '';
	return request<EndpointDetail>(`/endpoints/${owner}/${project}/${name}${params}`);
}

export function getEndpointDag(
	owner: string,
	project: string,
	name: string,
	ref?: string,
	format: string = 'mermaid'
) {
	const params = new URLSearchParams({ format });
	if (ref) params.set('ref', ref);
	return request<DagResponse>(`/endpoints/${owner}/${project}/${name}/dag?${params}`);
}

export function fetchEndpoint(
	owner: string,
	project: string,
	endpoint: string,
	params?: Record<string, string>,
	ref?: string
) {
	const searchParams = new URLSearchParams();
	if (params) {
		for (const [k, v] of Object.entries(params)) {
			searchParams.set(k, v);
		}
	}
	// Set ref after params so it cannot be overridden by a param named "ref"
	if (ref) searchParams.set('ref', ref);
	const qs = searchParams.toString();
	return rawRequest(`/fetch/${owner}/${project}/${endpoint}${qs ? '?' + qs : ''}`);
}

// ── Commits ──────────────────────────────────────────────────

export function listCommits(owner: string, project: string, limit: number = 50) {
	return request<CommitSummary[]>(`/commits/${owner}/${project}?limit=${limit}`);
}

export function getCommit(owner: string, project: string, sha: string) {
	return request<CommitDetail>(`/commits/${owner}/${project}/${sha}`);
}

// ── Secrets ──────────────────────────────────────────────────

export function listSecrets(owner: string, project: string) {
	return request<SecretInfo[]>(`/secrets/${owner}/${project}`);
}

export function setSecret(owner: string, project: string, name: string, value: string) {
	return request<{ name: string; version_id: string; created: boolean }>(
		`/secrets/${owner}/${project}`,
		{
			method: 'POST',
			body: JSON.stringify({ name, value })
		}
	);
}

export function deleteSecret(owner: string, project: string, name: string) {
	return request<void>(`/secrets/${owner}/${project}/${name}`, { method: 'DELETE' });
}

export { ApiError };
