// ── Auth ──────────────────────────────────────────────────────

export interface UserInfo {
	id: string;
	username: string;
	email: string | null;
	avatar_url: string | null;
	is_admin: boolean;
}

// ── Admin ─────────────────────────────────────────────────────

export interface AdminJob {
	id: string;
	project_id: string;
	endpoint_name: string;
	commit_id: string;
	status: string;
	error_message: string | null;
	created_by: string | null;
	created_at: string;
	started_at: string | null;
	completed_at: string | null;
}

export interface AdminRateLimits {
	global_max_concurrent: number;
	per_user_max_concurrent: number;
	active_jobs_global: number;
}

export interface AdminUser {
	id: string;
	username: string;
	display_name: string | null;
	email: string | null;
	avatar_url: string | null;
	is_admin: boolean;
	created_at: string;
}

export interface DeviceCodeResponse {
	device_code: string;
	user_code: string;
	verification_uri: string;
	expires_in: number;
	interval: number;
}

export interface AuthResponse {
	access_token: string | null;
	token_type: string | null;
	user: UserInfo | null;
	pending: boolean;
}

export interface ApiErrorResponse {
	error: string;
	message: string;
}

// ── Projects ─────────────────────────────────────────────────

export interface ProjectInfo {
	owner: string;
	slug: string;
	description: string | null;
	visibility: string;
	created_at: string;
	updated_at: string;
}

export interface ProjectDetail extends ProjectInfo {
	commit_count: number;
	refs: RefInfo[];
	collaborators: CollaboratorInfo[];
}

export interface RefInfo {
	name: string;
	ref_type: string;
	commit_sha: string | null;
}

export interface CollaboratorInfo {
	username: string;
	role: string;
}

// ── Data Atoms ───────────────────────────────────────────────

export interface DataAtom {
	id: string;
	name: string;
	hash: string;
	content_type: string;
	byte_size: number;
	yanked: boolean;
	created_at: string;
}

export interface DataAtomDetail extends DataAtom {
	uploaded_by: string;
	yank_reason: string | null;
	yanked_at: string | null;
	metadata: Record<string, unknown>;
}

export interface MetadataEntry {
	field: string;
	value: unknown;
	set_by: string;
	created_at: string;
}

// ── Collections ──────────────────────────────────────────────

export interface CollectionInfo {
	id: string;
	name: string;
	yanked: boolean;
	created_at: string;
	latest_version: number | null;
	member_count: number | null;
}

export interface CollectionDetail {
	id: string;
	name: string;
	yanked: boolean;
	yank_reason: string | null;
	yanked_at: string | null;
	created_at: string;
	version: VersionDetail | null;
}

export interface VersionDetail {
	version_number: number;
	hash: string;
	created_by: string;
	created_at: string;
	members: MemberInfo[];
}

export interface MemberInfo {
	member_type: string;
	member_ref: string;
	member_hash: string;
	ordinal: number;
}

export interface VersionLogEntry {
	version_number: number;
	hash: string;
	created_by: string;
	created_at: string;
}

export interface FlattenedAtom {
	name: string;
	hash: string;
	path: string[];
}

// ── Endpoints ────────────────────────────────────────────────

export interface EndpointSummary {
	name: string;
	description: string | null;
	params: ParamSummary[];
	node_count: number;
}

export interface EndpointDetail {
	name: string;
	description: string | null;
	commit_sha: string;
	params: ParamDetail[];
	nodes: Record<string, NodeDetail>;
	edges: EdgeDetail[];
}

export interface ParamSummary {
	name: string;
	type: string;
	description: string | null;
	default: unknown | null;
}

export interface ParamDetail extends ParamSummary {
	min: number | null;
	max: number | null;
	enum: unknown[] | null;
	binds: string;
}

export interface NodeDetail {
	transform: string;
	params: Record<string, unknown>;
	machine: string | null;
}

export interface EdgeDetail {
	from: string;
	to: string;
}

export interface DagResponse {
	format: string;
	content: string;
}

// ── Commits ──────────────────────────────────────────────────

export interface CommitSummary {
	id: string;
	git_commit_sha: string;
	git_provider: string;
	git_repo: string;
	message: string | null;
	pushed_by: string;
	created_at: string;
}

export interface CommitDetail extends CommitSummary {
	ozzy_toml_hash: string;
	environments: Record<string, unknown>;
	transforms: Record<string, unknown>;
	endpoints: Record<string, unknown>;
}

// ── Secrets ──────────────────────────────────────────────────

export interface SecretInfo {
	name: string;
	version_id: string;
	created_at: string;
	updated_at: string;
}
