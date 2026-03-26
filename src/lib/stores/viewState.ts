import { writable } from 'svelte/store';

export const currentView = writable('workbench'); // 'workbench' or 'config'
export const sidebarWidth = writable(320);
export const historyHeight = writable(400);
export const dialogueHeight = writable(250);
export const showCodeModal = writable(false);
export const selectedCode = writable('');
export const selectedTitle = writable('');
