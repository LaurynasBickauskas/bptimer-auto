<script lang="ts">
  import * as Tabs from "$lib/components/ui/tabs/index.js";
  import SettingsButton from "./settings-button.svelte";
  import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
  import * as path from '@tauri-apps/api/path';

  const SETTINGS_CATEGORY = "misc";
</script>

<Tabs.Content value={SETTINGS_CATEGORY}>
  <SettingsButton onclick={async () => await openPath(await path.appLogDir())} buttonLabel="Logs" label="Go to Logs Folder" description="Go to logs folder that contains all the logs for bptimer-auto. Use this file to report any bugs." />
  <SettingsButton onclick={async () => await openPath(await path.join(await path.appDataDir(), 'tauri-plugin-svelte'))} buttonLabel="Settings" label="Go to Settings Folder" description="Go to settings folder that contains all the setting files for bptimer-auto." />
  <SettingsButton onclick={async () => await revealItemInDir(await path.join(await path.appDataDir(), '.window-state.json'))} buttonLabel="Window Memory" label="Go to Window Memory Folder" description="Go to window memory folder that contains the window memory file for bptimer-auto. This file contains the memory of your window positions, etc." />
  <SettingsButton onclick={async () => await revealItemInDir(await path.join(await path.resourceDir(), 'bptimer-auto.exe'))} buttonLabel="App Install" label="Go to App Install location" description="Go to folder that contains the bptimer-auto installation." />
</Tabs.Content>
