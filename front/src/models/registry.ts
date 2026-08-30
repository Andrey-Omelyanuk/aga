import { Repository } from 'mobx-model-ui';
import { HttpClient } from '../api/http';
import { RESTAdapter } from '../api/adapter';
import { Project } from './project';
import { Workstation } from './workstation';
import { User } from './user';
import { AgentSet } from './agentset';
import { Chat } from './chat';

export const http = new HttpClient();

export const projectRepo: Repository<Project> = new Repository(
  Project.getModelDescriptor(),
  new RESTAdapter('/projects', http),
);
export const workstationRepo: Repository<Workstation> = new Repository(
  Workstation.getModelDescriptor(),
  new RESTAdapter('/workstations', http),
);
export const userRepo: Repository<User> = new Repository(
  User.getModelDescriptor(),
  new RESTAdapter('/users', http),
);
export const agentSetRepo: Repository<AgentSet> = new Repository(
  AgentSet.getModelDescriptor(),
  new RESTAdapter('/agent-sets', http),
);
export const chatRepo: Repository<Chat> = new Repository(
  Chat.getModelDescriptor(),
  new RESTAdapter('/chats', http),
);