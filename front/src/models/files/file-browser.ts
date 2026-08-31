import { makeAutoObservable } from 'mobx';
import http from '@/services/http';

export interface TreeEntry {
  name: string;
  path: string;
  kind: 'dir' | 'file';
}

export interface FileContent {
  contentType: string;
  text?: string;
  objectUrl?: string;
}

const LANG_MAP: Record<string, string> = {
  rs: 'rust',
  py: 'python',
  js: 'javascript',
  ts: 'typescript',
  tsx: 'typescript',
  jsx: 'javascript',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  hpp: 'cpp',
  java: 'java',
  go: 'go',
  rb: 'ruby',
  php: 'php',
  sh: 'bash',
  bash: 'bash',
  html: 'xml',
  htm: 'xml',
  css: 'css',
  scss: 'scss',
  json: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'ini',
  ini: 'ini',
  xml: 'xml',
  md: 'markdown',
  markdown: 'markdown',
  sql: 'sql',
  dockerfile: 'dockerfile',
  makefile: 'makefile',
  graphql: 'graphql',
  proto: 'protobuf',
  txt: 'plaintext',
};

export function langFor(path: string): string {
  const ext = (path.split('.').pop() ?? '').toLowerCase();
  return LANG_MAP[ext] ?? 'plaintext';
}

export class FileBrowser {
  workstationId: number | null = null;
  entries: TreeEntry[] = [];
  currentPath: string | null = null;
  content: FileContent | null = null;
  loading = false;
  private revokeUrl: string | null = null;

  constructor() {
    makeAutoObservable(this);
  }

  async selectWorkstation(id: number | null): Promise<void> {
    this.revoke();
    this.workstationId = id;
    this.currentPath = null;
    this.content = null;
    await this.loadRoot();
  }

  async loadRoot(): Promise<void> {
    this.entries = [];
    if (this.workstationId === null) return;
    await this.loadDir('');
  }

  async loadDir(relPath: string): Promise<void> {
    if (this.workstationId === null) return;
    const qs = relPath ? `?path=${encodeURIComponent(relPath)}` : '';
    const response = await http.get<{ entries: TreeEntry[] }>(
      `/workstations/${this.workstationId}/tree${qs}`,
    );
    this.entries = response.data.entries;
  }

  async toggleDir(relPath: string, expanded: boolean): Promise<void> {
    if (expanded) await this.loadDir(relPath);
  }

  async openFile(entry: TreeEntry): Promise<void> {
    if (this.workstationId === null) return;
    this.revoke();
    this.currentPath = entry.path;
    this.content = null;
    this.loading = true;
    try {
      const response = await http.get(
        `/workstations/${this.workstationId}/file?path=${encodeURIComponent(entry.path)}`,
        { responseType: 'blob' },
      );
      const contentType = String(response.headers['content-type'] ?? 'text/plain');
      const blob: Blob = response.data;
      if (
        contentType.startsWith('image/') ||
        contentType.startsWith('video/') ||
        contentType.startsWith('audio/')
      ) {
        const url = URL.createObjectURL(blob);
        this.revokeUrl = url;
        this.content = { contentType, objectUrl: url };
      } else {
        this.content = { contentType, text: await blob.text() };
      }
    } catch (e) {
      this.content = { contentType: 'text/plain', text: undefined };
    } finally {
      this.loading = false;
    }
  }

  private revoke(): void {
    if (this.revokeUrl) {
      URL.revokeObjectURL(this.revokeUrl);
      this.revokeUrl = null;
    }
  }
}

export const fileBrowser = new FileBrowser();