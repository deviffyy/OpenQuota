export interface UpdaterMetadata {
  version: string;
  notes: string;
  pub_date: string;
  platforms: Record<string, { url: string; signature: string }>;
}

export interface GithubReleaseMetadata {
  body?: string;
  created_at?: string;
  assets: Array<{ id: number; name: string }>;
}

export function createUpdaterMetadata(
  release: GithubReleaseMetadata,
  signatures: Record<string, string>,
  repository: string,
  tag: string,
  version: string,
): UpdaterMetadata;
