import { invoke } from '@tauri-apps/api/core';
import type { BrowserSupport, ProbeInfo, Settings, UpdateChannel } from './types';

export const getSettings = () => invoke<Settings>('get_settings');
export const saveSettings = (settings: Settings) => invoke<void>('save_settings', { settings });
export const configPath = () => invoke<string>('config_path');
export const browserSupport = () => invoke<BrowserSupport[]>('browser_support');
export const ytdlpVersion = () => invoke<string>('ytdlp_version');
export const checkForUpdates = (channel: UpdateChannel) =>
  invoke<string>('check_for_updates', { channel });
export const probe = (url: string) => invoke<ProbeInfo>('probe', { url });
export const download = (settings: Settings, saveFolder: string, url: string) =>
  invoke<void>('download', { settings, saveFolder, url });
export const cancel = (saveFolder: string) => invoke<void>('cancel', { saveFolder });
