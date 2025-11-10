import { version } from '@tauri-apps/plugin-os';
import { RuneStore } from '@tauri-store/svelte';

const IS_WIN_11 = parseInt(version().split(".")[2] || "0", 10) >= 22000;

const DEFAULT_SETTINGS = {
  accessibility: {
    blur: !IS_WIN_11,
    transparency: false,
  },
  shortcuts: {
    showLiveMeter: "",
    hideLiveMeter: "",
    toggleLiveMeter: "",
    enableClickthrough: "",
    disableClickthrough: "",
    toggleClickthrough: "",
    hardReset: "",
    markCurrentMonsterDead: "",
  },
  integration: {
    bptimerUI: true,
  }
};

// We need flattened settings for every update to be able to auto-detect new changes
const RUNE_STORE_OPTIONS = { autoStart: true, saveOnChange: true };

export const SETTINGS = {
  accessibility: new RuneStore(
    'accessibility',
    DEFAULT_SETTINGS.accessibility,
    RUNE_STORE_OPTIONS
  ),
  shortcuts: new RuneStore(
    'shortcuts',
    DEFAULT_SETTINGS.shortcuts,
    RUNE_STORE_OPTIONS
  ),
  integration: new RuneStore(
    'integration',
    DEFAULT_SETTINGS.integration,
    RUNE_STORE_OPTIONS
  ),
};
