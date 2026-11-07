import { makeAutoObservable, runInAction } from 'mobx';
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
  // Номер последнего запроса файла: ответ устаревшего (пользователь уже
  // открыл другой файл или станцию) отбрасывается, его blob-URL освобождается.
  private fileRequest = 0;

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

  /** Содержимое папки проекта станции (`''` — корень). */
  async listDir(relPath: string): Promise<TreeEntry[]> {
    if (this.workstationId === null) return [];
    const qs = relPath ? `?path=${encodeURIComponent(relPath)}` : '';
    const response = await http.get<{ entries: TreeEntry[] }>(
      `/workstations/${this.workstationId}/tree${qs}`,
    );
    return response.data.entries;
  }

  async loadDir(relPath: string): Promise<void> {
    const ws = this.workstationId;
    const entries = await this.listDir(relPath);
    runInAction(() => {
      if (this.workstationId === ws) this.entries = entries;
    });
  }

  async openFile(entry: TreeEntry): Promise<void> {
    if (this.workstationId === null) return;
    this.revoke();
    this.currentPath = entry.path;
    this.content = null;
    this.loading = true;
    const request = ++this.fileRequest;
    const current = () => request === this.fileRequest;
    let content: FileContent;
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
        if (!current()) return;
        content = { contentType, objectUrl: URL.createObjectURL(blob) };
      } else {
        content = { contentType, text: await blob.text() };
      }
    } catch {
      content = { contentType: 'text/plain', text: undefined };
    }
    runInAction(() => {
      if (!current()) {
        if (content.objectUrl) URL.revokeObjectURL(content.objectUrl);
        return;
      }
      this.revokeUrl = content.objectUrl ?? null;
      this.content = content;
      this.loading = false;
    });
  }

  private revoke(): void {
    if (this.revokeUrl) {
      URL.revokeObjectURL(this.revokeUrl);
      this.revokeUrl = null;
    }
  }
}

export const fileBrowser = new FileBrowser();