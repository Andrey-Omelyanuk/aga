import { AppStore } from './models/store';
import { FileBrowser } from './models/file-browser';

export const store = new AppStore();
export const fileBrowser = new FileBrowser();
void store.init();

export type { AppStore, TabName } from './models/store';
export type { FileBrowser } from './models/file-browser';
export type {
  TreeEntry,
} from './models/file-browser';
export * from './models/chat';