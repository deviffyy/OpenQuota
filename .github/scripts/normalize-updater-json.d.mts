export interface UpdaterMetadata {
  platforms: Record<string, { url: string; signature: string }>;
  [key: string]: unknown;
}

export interface GithubReleaseMetadata {
  assets: Array<{ id: number; name: string }>;
}

export function normalizeUpdaterMetadata<T extends UpdaterMetadata>(
  update: T,
  release: GithubReleaseMetadata,
  repository: string,
  tag: string,
): T;
