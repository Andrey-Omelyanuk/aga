import { makeAutoObservable, runInAction } from 'mobx';
import { API_BASE, TOKEN_KEY } from '../api/http';
import {
  chatRepo,
  projectRepo,
  workstationRepo,
  userRepo,
  agentSetRepo,
  http,
} from './registry';
import { Project } from './project';
import { Workstation } from './workstation';
import { User } from './user';
import { AgentSet } from './agentset';
import { Chat } from './chat';

export type TabName =
  | 'projects'
  | 'workstations'
  | 'sessions'
  | 'personnel'
  | 'files'
  | 'chat';

const POLL_INTERVAL_MS = 2000;
const ACTIVE_PROJECT_KEY = 'aga_active_project';

export class AppStore {
  projects: Project[] = [];
  workstations: Workstation[] = [];
  users: User[] = [];
  agents: Array<{ name: string }> = [];
  chats: Chat[] = [];

  currentChatId: number | null = null;
  currentChat: Chat | null = null;
  activeProjectId: number | null = null;
  showLogin = false;

  private pollTimer: ReturnType<typeof setInterval> | null = null;

  constructor() {
    makeAutoObservable(this);
    this.activeProjectId = this.readActiveProjectId();
  }

  get loginUrl(): string {
    return `${API_BASE}/auth/login`;
  }

  get activeProject(): Project | null {
    return this.projects.find((p) => p.id === this.activeProjectId) ?? null;
  }

  /// Воркстейшны, на которых можно открыть сессию: только готовые и при этом
  /// свободные или занятые текущим проектом (глобальный фильтр).
  get sessionWorkstations(): Workstation[] {
    return this.workstations.filter(
      (ws) =>
        ws.isReady &&
        (ws.isFree ||
          (this.activeProjectId !== null && ws.project_id === this.activeProjectId)),
    );
  }

  projectName(id: number): string {
    const p = this.projects.find((x) => x.id === id);
    return p ? p.git_url : `Проект #${id}`;
  }

  setActiveProject(id: number | null): void {
    this.activeProjectId = id;
    if (id === null) {
      localStorage.removeItem(ACTIVE_PROJECT_KEY);
    } else {
      localStorage.setItem(ACTIVE_PROJECT_KEY, String(id));
    }
  }

  private readActiveProjectId(): number | null {
    const raw = localStorage.getItem(ACTIVE_PROJECT_KEY);
    if (raw === null) return null;
    const id = Number(raw);
    return Number.isFinite(id) ? id : null;
  }

  async init(): Promise<void> {
    this.readTokenFromHash();
    http.setOnUnauthorized(() => runInAction(() => (this.showLogin = true)));
    await Promise.all([
      this.loadProjects(),
      this.loadWorkstations(),
      this.loadChats(),
      this.loadAgents(),
    ]);
  }

  private readTokenFromHash(): void {
    const match = location.hash.match(/^#token=(.+)$/);
    if (match) {
      localStorage.setItem(TOKEN_KEY, match[1]);
      if (window.history.replaceState) {
        window.history.replaceState(null, '', location.pathname + location.search);
      }
    }
    if (localStorage.getItem(TOKEN_KEY)) this.showLogin = false;
  }

  async ensureLoaded(tab: TabName): Promise<void> {
    switch (tab) {
      case 'projects':
        await this.loadProjects();
        break;
      case 'workstations':
        await this.loadWorkstations();
        break;
      case 'sessions':
        await Promise.all([this.loadChats(), this.loadWorkstations()]);
        break;
      case 'personnel':
        await this.loadUsers();
        break;
      case 'files':
        await this.loadWorkstations();
        break;
      case 'chat':
        await this.loadChats();
        break;
    }
  }

  async loadProjects(): Promise<void> {
    const items = await projectRepo.load(Project.getQuery({}));
    runInAction(() => {
      this.projects = items;
      const stillExists = items.some((p) => p.id === this.activeProjectId);
      if (!stillExists) {
        this.setActiveProject(items.length > 0 ? items[0].id : null);
      }
    });
  }

  async createProject(gitUrl: string): Promise<void> {
    await projectRepo.create(new Project({ git_url: gitUrl }));
    await this.loadProjects();
  }

  async deleteProject(id: number): Promise<void> {
    const project = this.projects.find((p) => p.id === id);
    if (project) await projectRepo.delete(project);
    await this.loadProjects();
  }

  async loadWorkstations(): Promise<void> {
    const items = await workstationRepo.load(Workstation.getQuery({}));
    runInAction(() => (this.workstations = items));
  }

  async loadUsers(): Promise<void> {
    const items = await userRepo.load(User.getQuery({}));
    runInAction(() => (this.users = items));
  }

  async loadAgents(): Promise<void> {
    const sets = await agentSetRepo.load(AgentSet.getQuery({}));
    const seen = new Set<string>();
    const names: Array<{ name: string }> = [];
    for (const set of sets) {
      for (const agent of set.agents ?? []) {
        if (seen.has(agent.name)) continue;
        seen.add(agent.name);
        names.push({ name: agent.name });
      }
    }
    runInAction(() => (this.agents = names));
  }

  async loadChats(): Promise<void> {
    const items = await chatRepo.load(Chat.getQuery({}));
    runInAction(() => (this.chats = items));
    if (this.currentChatId && !items.find((c) => c.id === this.currentChatId)) {
      this.selectChat(this.currentChatId);
    }
  }

  async openWorkstationSession(workstationId: number, title: string): Promise<void> {
    await workstationRepo.action(
      this.wsById(workstationId),
      'session',
      { title: title || 'Новая сессия' },
    );
    await this.loadChats();
  }

  async releaseWorkstation(id: number): Promise<void> {
    await workstationRepo.action(this.wsById(id), 'release', {});
    await this.loadWorkstations();
  }

  async occupyWorkstation(id: number): Promise<void> {
    if (this.activeProjectId === null) return;
    await workstationRepo.action(this.wsById(id), 'switch', {
      project_id: this.activeProjectId,
    });
    await this.loadWorkstations();
  }

  /// Экземпляр воркстейшна из кэша: конструировать `new Workstation({ id })`
  /// нельзя — id уже в кэше, и cache.inject упадёт «already exist».
  private wsById(id: number): Workstation {
    const ws = this.workstations.find((w) => w.id === id);
    if (ws) return ws;
    throw new Error(`воркстейшн #${id} не загружен`);
  }

  async createChat(title = 'Новая сессия'): Promise<Chat> {
    const chat = new Chat({ title });
    await chatRepo.create(chat);
    await this.loadChats();
    return chatRepo.get(chat.id);
  }

  async closeChat(id: number): Promise<void> {
    const chat = this.chats.find((c) => c.id === id);
    if (!chat) throw new Error(`чат #${id} не загружен`);
    await chatRepo.action(chat, 'close', {});
    await this.loadChats();
    if (this.currentChatId === id) this.selectChat(null);
  }

  async selectChat(id: number | null): Promise<void> {
    this.currentChatId = id;
    if (id === null) {
      this.currentChat = null;
      this.stopPolling();
      return;
    }
    await this.loadChat(id);
    this.startPolling();
  }

  async refreshCurrentChat(): Promise<void> {
    if (!this.currentChatId) return;
    await this.loadChat(this.currentChatId);
  }

  private async loadChat(id: number): Promise<void> {
    const chat = await chatRepo.get(id);
    runInAction(() => (this.currentChat = chat));
  }

  async sendMessage(body: string): Promise<void> {
    if (!this.currentChatId) return;
    const chat = this.currentChat ?? this.chats.find((c) => c.id === this.currentChatId);
    if (!chat) throw new Error('чат не загружен');
    const data = await chatRepo.action(chat, 'messages', { body });
    if (this.currentChat) {
      this.currentChat.messages.push(data.message);
    }
    await this.loadChats();
  }

  async artifactsOf(messageId: number): Promise<unknown> {
    return http.json(`/messages/${messageId}/artifacts`);
  }

  private startPolling(): void {
    this.stopPolling();
    this.pollTimer = setInterval(() => {
      void this.refreshCurrentChat();
    }, POLL_INTERVAL_MS);
  }

  private stopPolling(): void {
    if (this.pollTimer) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
  }
}