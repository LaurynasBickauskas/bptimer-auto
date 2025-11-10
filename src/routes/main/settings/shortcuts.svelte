<script lang="ts">
  import { onDestroy } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { unregister } from "@tauri-apps/plugin-global-shortcut";

  import * as Tabs from "$lib/components/ui/tabs/index.js";
  import * as Item from "$lib/components/ui/item/index.js";
  import * as Alert from "$lib/components/ui/alert/index.js";
  import { Button } from "$lib/components/ui/button/index.js";

  import { SETTINGS } from "$lib/settings-store";
  import { registerShortcut } from "./shortcuts.js";
  import type { BaseInput, BaseInputs } from "./settings.js";

  let editingId: string | null = $state(null);

  const modifierOrder = ["ctrl", "shift", "alt", "meta"];
  const MODIFIERS = new SvelteSet(modifierOrder);
  const activeMods = new SvelteSet<string>();
  let mainKey: string | null = $state(null);

  /** Normalize key names */
  const normalizeKey = (key: string): string =>
    (
      ({
        control: "ctrl",
        meta: "meta",
        alt: "alt",
        shift: "shift",
      }) as Record<string, string>
    )[key.toLowerCase()] ?? key.toLowerCase();

  /** Build the display string of the in-progress shortcut */
  function currentShortcutString(): string {
    const mods = modifierOrder.filter((m) => activeMods.has(m));
    return mainKey ? [...mods, mainKey].join("+") : mods.join("+");
  }

  function startEdit(shortcut: BaseInput) {
    stopEdit();
    editingId = shortcut.id;
    activeMods.clear();
    mainKey = null;

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
  }

  function stopEdit() {
    window.removeEventListener("keydown", handleKeyDown);
    window.removeEventListener("keyup", handleKeyUp);
    activeMods.clear();
    mainKey = null;
    editingId = null;
  }

  function handleKeyDown(e: KeyboardEvent) {
    e.preventDefault();
    const k = normalizeKey(e.key);

    if (MODIFIERS.has(k)) {
      activeMods.add(k);
      return;
    }

    mainKey = k;
  }

  function handleKeyUp(e: KeyboardEvent) {
    e.preventDefault();
    const k = normalizeKey(e.key);

    if (MODIFIERS.has(k)) {
      activeMods.delete(k);
      stopEdit();
      return;
    }

    if (mainKey) {
      const shortcutKey = currentShortcutString();

      const hasMain = !!mainKey;
      if (!hasMain) return;

      const cmd = inputs.find((c) => c.id === editingId);
      if (cmd) {
        unregister(SETTINGS.shortcuts.state[cmd.id]);
        SETTINGS.shortcuts.state[cmd.id] = shortcutKey;
        registerShortcut(cmd.id, shortcutKey);
      }
      stopEdit();
    }
  }

  async function clearShortcut(shortcut: BaseInput, e: MouseEvent) {
    e.preventDefault();
    const existing = SETTINGS.shortcuts.state[shortcut.id];
    if (existing) {
      SETTINGS.shortcuts.state[shortcut.id] = "";
      await unregister(existing);
    }
  }

  onDestroy(stopEdit);

  const SETTINGS_CATEGORY = "shortcuts";

  let inputs: BaseInputs = [
    {
      id: "markCurrentMonsterDead",
      label: "Mark Current Monster Dead",
    },
    {
      id: "toggleLiveMeter",
      label: "Toggle Live Meter",
    },
    {
      id: "enableClickthrough",
      label: "Enable Clickthrough",
    },
    {
      id: "disableClickthrough",
      label: "Disable Clickthrough",
    },
    {
      id: "toggleClickthrough",
      label: "Toggle Clickthrough",
    },
    {
      id: "hardReset",
      label: "TEMP FIX: Hard Reset",
    },
  ];
</script>

<Tabs.Content value={SETTINGS_CATEGORY}>
  <Alert.Root>
    <Alert.Title>Right click to clear shortcuts</Alert.Title>
  </Alert.Root>
  {#each inputs as input (input.id)}
    <Item.Root>
      <Item.Content>
        <Item.Title>{input.label}</Item.Title>
      </Item.Content>
      <Item.Actions>
        <Button variant="outline" class="uppercase" onclick={() => startEdit(input)} oncontextmenu={(e: MouseEvent) => clearShortcut(input, e)}>
          {#if editingId === input.id}
            {currentShortcutString() || "Press keys"}...
          {:else}
            {SETTINGS.shortcuts.state[input.id] || "Unbound"}
          {/if}
        </Button>
      </Item.Actions>
    </Item.Root>
  {/each}
</Tabs.Content>
