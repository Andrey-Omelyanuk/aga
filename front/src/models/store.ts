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

export class AppStore {
  projects: Project[] = [];
  workstations: Workstation[] = [];
  users: User[] = [];
  agents: Array<{ name: string }> = [];
  chats: Chat[] = [];

  currentChatId: number | null = null;
  currentChat: Chat | null = null;
  activeTab: TabName = 'projects';
  showLogin = false;

  private pollTimer: ReturnType<typeof setInterval> | null = null;

  constructor() {
    makeAutoObservable(this);
  }

  get loginUrl(): string {
    return `${API_BASE}/auth/login`;
  }

  setActiveTab(tab: TabName): void {
    this.activeTab = tab;
    void this.ensureLoaded(tab);
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

  private async ensureLoaded(tab: TabName): Promise<void> {
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
    runInAction(() => (this.projects = items));
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
      new Workstation({ id: workstationId }),
      'session',
      { title: title || 'Новая сессия' },
    );
    await this.loadChats();
  }

  async createChat(title = 'Новая сессия'): Promise<Chat> {
    const chat = new Chat({ title });
    await chatRepo.create(chat);
    await this.loadChats();
    return chatRepo.get(chat.id);
  }

  async closeChat(id: number): Promise<void> {
    await chatRepo.action(new Chat({ id }), 'close', {});
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
    const data = await chatRepo.action(
      new Chat({ id: this.currentChatId }),
      'messages',
      { body },
    );
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