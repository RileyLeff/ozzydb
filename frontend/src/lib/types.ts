export interface UserInfo {
	id: string;
	username: string;
	email: string | null;
	avatar_url: string | null;
}

export interface ProjectInfo {
	id: string;
	owner: string;
	slug: string;
	description: string | null;
	visibility: string;
	default_branch: string;
	created_at: string;
	updated_at: string;
}

export interface CommitInfo {
	hash: string;
	parent_hashes: string[];
	author: string;
	message: string;
	created_at: string;
}

export interface RefInfo {
	name: string;
	ref_type: 'branch' | 'tag';
	commit_hash: string;
	updated_at: string;
}

export interface ListRefsResponse {
	branches: RefInfo[];
	tags: RefInfo[];
}

export interface CollaboratorInfo {
	username: string;
	permission: 'read' | 'write' | 'admin';
	added_at: string;
}

export interface DataSourceInfo {
	name: string;
	hash: string;
	schema_hash: string;
	schema: unknown;
	row_count: number | null;
	byte_size: number | null;
}

export interface TransformInfo {
	name: string;
	hash: string;
	runtime: string;
	source_path: string;
	function_name: string;
	lockfile_hash: string | null;
	params_schema: unknown | null;
	input_schema: unknown | null;
	output_schema: unknown | null;
	reproducible: boolean;
}

export interface EndpointInfo {
	name: string;
	description: string | null;
	definition: EndpointDefinition;
}

export interface EndpointDefinition {
	name: string;
	description: string | null;
	nodes: EndpointNode[];
	edges: EndpointEdge[];
}

export interface EndpointNode {
	node_name: string;
	transform_name: string;
	params: Record<string, unknown> | null;
}

export interface EndpointEdge {
	source_type: 'data_source' | 'node' | 'external';
	source_ref: string;
	target_node: string;
	target_input: string;
	external_owner?: string;
	external_project?: string;
	external_endpoint?: string;
	external_ref?: string;
}

export interface DagResponse {
	owner: string;
	project: string;
	ref_name: string;
	commit_hash: string;
	data_sources: DataSourceInfo[];
	transforms: TransformInfo[];
	endpoints: EndpointInfo[];
}

export interface EndpointsAtRefResponse {
	ref_name: string;
	commit_hash: string;
	endpoints: EndpointInfo[];
}

export interface TransformsAtRefResponse {
	ref_name: string;
	commit_hash: string;
	transforms: TransformInfo[];
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
