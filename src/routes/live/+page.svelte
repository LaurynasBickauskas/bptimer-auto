<script lang="ts">
  import { onMount } from "svelte";
  import { commands } from "$lib/bindings";
  import type { Result } from "$lib/bindings";

  type MobHpData = {
    remote_id: string;
    server_id: number;
    hp_percent: number;
  };

  type MobHpUpdate = MobHpData;

  type MobChannelStatusItem = {
    channel_number: number;
    last_hp: number | null;
    mob: string;
  };

  type CrowdsourcedMonster = {
    name: string;
    id: number;
    remote_id: string | null;
  };

  type CrowdsourcedMonsterOption = {
    name: string;
    id: number;
    remote_id: string;
  };

  const BPTIMER_BASE_URL = "https://db.bptimer.com";
  const MOB_CHANNEL_STATUS_ENDPOINT = "/api/collections/mob_channel_status/records";
  const REALTIME_ENDPOINT = "/api/realtime";
  const MOB_COLLECTION_AUTH_TOKEN =
    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJjb2xsZWN0aW9uSWQiOiJfcGJfdXNlcnNfYXV0aF8iLCJleHAiOjE3NjMxMTYwMTIsImlkIjoibmhtc2s3Z2g1ODhieXc3IiwicmVmcmVzaGFibGUiOnRydWUsInR5cGUiOiJhdXRoIn0.I81wYPhG0u8IUcQWZGBFsKS5abnQ1JOtFjIcjqkyO0A";

  const RESEED_INTERVAL_MS = 120_000;
  const STORE_ENTRY_TTL_MS = 300_000;
  const SSE_RETRY_DELAY_MS = 3_000;

  const commandsExtended = commands as typeof commands & {
    getCrowdsourcedMonster: () => Promise<CrowdsourcedMonster | null>;
    getCrowdsourcedMonsterOptions: () => Promise<CrowdsourcedMonsterOption[]>;
    setCrowdsourcedMonsterRemote: (remoteId: string) => Promise<Result<null, string>>;
    getLocalPlayerLine: () => Promise<Result<number | null, string>>;
    markCurrentCrowdsourcedLineDead: () => Promise<Result<null, string>>;
  };

  let monsterOptions: CrowdsourcedMonsterOption[] = $state([]);
  let currentMonster: CrowdsourcedMonster | null = $state(null);
  let selectedRemoteId: string | null = $state(null);
  let mobHpData: MobHpData[] = $state([]);
  let currentLineId: number | null = $state(null);

  type HpChangeRecord = { hp: number; timestamp: number };

  const mobHpStore = new Map<string, Map<number, { data: MobHpData; timestamp: number }>>();
  const mobHpLastChange = new Map<string, HpChangeRecord>();
  const lastSeedTimestamps = new Map<string, number>();

  let activeRemoteId: string | null = null;
  let streamActive = false;
  let streamAbortController: AbortController | null = null;
  let streamRunner: Promise<void> | null = null;

  let fetchInterval: ReturnType<typeof setInterval> | null = null;
  let reseedInterval: ReturnType<typeof setInterval> | null = null;
  let cleanupInterval: ReturnType<typeof setInterval> | null = null;

  let seedNonce = 0;
  const textDecoder = new TextDecoder();

  function mobKey(entry: MobHpData) {
    return `${entry.remote_id}:${entry.server_id}`;
  }

  function updateMobLastChange(entries: MobHpData[]) {
    const now = Date.now();
    const seen = new Set<string>();

    for (const entry of entries) {
      const key = mobKey(entry);
      seen.add(key);
      const record = mobHpLastChange.get(key);
      if (!record || record.hp !== entry.hp_percent) {
        mobHpLastChange.set(key, { hp: entry.hp_percent, timestamp: now });
      }
    }

    for (const key of Array.from(mobHpLastChange.keys())) {
      if (!seen.has(key)) {
        mobHpLastChange.delete(key);
      }
    }
  }

  function filterStaleEntries(entries: MobHpData[]) {
    const now = Date.now();
    const STALE_HP_THRESHOLD = 30;
    const STALE_HP_DURATION_MS = 30_000;

    return entries.filter((entry) => {
      if (entry.hp_percent > STALE_HP_THRESHOLD) {
        return true;
      }
      const record = mobHpLastChange.get(mobKey(entry));
      if (!record) {
        return true;
      }
      return now - record.timestamp < STALE_HP_DURATION_MS;
    });
  }

  function clampPercent(value: number) {
    return Math.min(100, Math.max(0, value));
  }

  function barClass(percent: number) {
    if (percent === 0) return "bg-neutral-700";
    if (percent <= 30) return "bg-red-600/80";
    if (percent <= 60) return "bg-yellow-500/80";
    if (percent <= 99) return "bg-green-500/80";

    return "bg-green-500/20";
  }

  function refreshMobHpData() {
    if (!activeRemoteId) {
      mobHpData = [];
      return;
    }

    const serverMap = mobHpStore.get(activeRemoteId);
    if (!serverMap) {
      mobHpData = [];
      return;
    }

    const entries = Array.from(serverMap.values()).map((entry) => entry.data);
    updateMobLastChange(entries);
    mobHpData = filterStaleEntries(entries);
  }

  function upsertMobEntry(remoteId: string, serverId: number, hpPercent: number) {
    const now = Date.now();
    let serverMap = mobHpStore.get(remoteId);
    if (!serverMap) {
      serverMap = new Map();
      mobHpStore.set(remoteId, serverMap);
    }

    const data: MobHpData = {
      remote_id: remoteId,
      server_id: serverId,
      hp_percent: clampPercent(hpPercent),
    };

    serverMap.set(serverId, { data, timestamp: now });
    if (serverId !== 0) {
      serverMap.delete(0);
    }

    if (remoteId === activeRemoteId) {
      refreshMobHpData();
    }
  }

  function applySeed(remoteId: string, items: MobChannelStatusItem[]) {
    const now = Date.now();
    let serverMap = mobHpStore.get(remoteId);
    if (!serverMap) {
      serverMap = new Map();
      mobHpStore.set(remoteId, serverMap);
    } else {
      serverMap.clear();
    }

    if (items.length === 0) {
      const data: MobHpData = { remote_id: remoteId, server_id: 0, hp_percent: 100 };
      serverMap.set(0, { data, timestamp: now });
    } else {
      for (const item of items) {
        const hpPercent = item.last_hp ?? 100;
        const data: MobHpData = {
          remote_id: remoteId,
          server_id: item.channel_number,
          hp_percent: clampPercent(hpPercent),
        };
        serverMap.set(item.channel_number, { data, timestamp: now });
        if (item.channel_number !== 0) {
          serverMap.delete(0);
        }
      }
    }

    if (remoteId === activeRemoteId) {
      refreshMobHpData();
    }
  }

  function cleanupMobHpStore(maxAgeMs = STORE_ENTRY_TTL_MS) {
    const now = Date.now();

    for (const [remoteId, serverMap] of mobHpStore) {
      for (const [serverId, entry] of serverMap) {
        if (now - entry.timestamp > maxAgeMs) {
          serverMap.delete(serverId);
        }
      }

      if (serverMap.size === 0) {
        mobHpStore.delete(remoteId);
      }
    }

    if (activeRemoteId && !mobHpStore.has(activeRemoteId)) {
      mobHpData = [];
    }
  }

  function switchActiveRemote(remoteId: string | null) {
    if (activeRemoteId === remoteId) {
      selectedRemoteId = remoteId;
      return;
    }

    if (activeRemoteId) {
      mobHpStore.delete(activeRemoteId);
    }

    activeRemoteId = remoteId;
    mobHpLastChange.clear();

    if (!remoteId) {
      selectedRemoteId = null;
      mobHpData = [];
      return;
    }

    if (!mobHpStore.has(remoteId)) {
      mobHpStore.set(remoteId, new Map());
    } else {
      mobHpStore.get(remoteId)?.clear();
    }

    selectedRemoteId = remoteId;
    refreshMobHpData();
  }

  async function ensureSeeded(remoteId: string, opts: { force?: boolean } = {}) {
    const { force = false } = opts;
    const lastSeed = lastSeedTimestamps.get(remoteId) ?? 0;
    if (!force && Date.now() - lastSeed < RESEED_INTERVAL_MS) {
      return;
    }

    const currentNonce = ++seedNonce;

    try {
      const items = await fetchMobChannelStatus(remoteId);
      if (seedNonce !== currentNonce) {
        return;
      }
      if (activeRemoteId !== remoteId) {
        return;
      }

      applySeed(remoteId, items);
      lastSeedTimestamps.set(remoteId, Date.now());
    } catch (error) {
      console.error("live/+page:ensureSeeded", { error, remoteId });
    }
  }

  async function fetchMobChannelStatus(remoteId: string, signal?: AbortSignal): Promise<MobChannelStatusItem[]> {
    const params = new URLSearchParams({
      page: "1",
      perPage: "200",
      skipTotal: "true",
      filter: `mob = '${remoteId}'`,
    });

    const response = await fetch(`${BPTIMER_BASE_URL}${MOB_CHANNEL_STATUS_ENDPOINT}?${params.toString()}`, {
      method: "GET",
      headers: {
        accept: "application/json",
        authorization: MOB_COLLECTION_AUTH_TOKEN,
      },
      signal,
    });

    if (!response.ok) {
      const body = await response.text();
      throw new Error(`Failed to seed mob state (${response.status}): ${body}`);
    }

    const payload = (await response.json()) as { items?: MobChannelStatusItem[] };
    return payload.items ?? [];
  }

  function setStreamActive(active: boolean) {
    if (streamActive === active) {
      return;
    }

    streamActive = active;

    if (active) {
      if (!streamAbortController) {
        streamAbortController = new AbortController();
        streamRunner = runSseLoop(streamAbortController.signal)
          .catch((error) => {
            if (!(error instanceof DOMException && error.name === "AbortError")) {
              console.error("live/+page:stream", { error });
            }
          })
          .finally(() => {
            if (streamAbortController?.signal.aborted) {
              streamAbortController = null;
            }
            streamRunner = null;
            streamActive = false;
          });
      }
    } else {
      streamAbortController?.abort();
      streamAbortController = null;
      streamRunner = null;
    }
  }

  async function runSseLoop(signal: AbortSignal) {
    while (!signal.aborted) {
      try {
        await streamOnce(signal);
      } catch (error) {
        if (signal.aborted) {
          return;
        }
        console.error("live/+page:runSseLoop", { error });
        await delay(SSE_RETRY_DELAY_MS, signal).catch(() => {});
      }
    }
  }

  async function streamOnce(signal: AbortSignal) {
    const response = await fetch(`${BPTIMER_BASE_URL}${REALTIME_ENDPOINT}`, {
      method: "GET",
      headers: {
        accept: "text/event-stream",
        "cache-control": "no-cache",
        pragma: "no-cache",
      },
      signal,
    });

    if (!response.ok) {
      const body = await response.text();
      throw new Error(`Failed to connect to realtime stream (${response.status}): ${body}`);
    }

    const reader = response.body?.getReader();
    if (!reader) {
      throw new Error("Realtime stream response did not include a readable body");
    }

    let buffer = "";
    let currentEvent: { eventType?: string; data?: string } | null = null;
    let subscribed = false;

    while (!signal.aborted) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      if (value) {
        buffer += textDecoder.decode(value, { stream: true });
      }

      let newlineIndex: number;
      while ((newlineIndex = buffer.indexOf("\n")) !== -1) {
        let line = buffer.slice(0, newlineIndex);
        buffer = buffer.slice(newlineIndex + 1);

        if (line.endsWith("\r")) {
          line = line.slice(0, -1);
        }

        if (line.length === 0) {
          if (currentEvent) {
            subscribed = await handleSseEvent(currentEvent, subscribed, signal);
          }
          currentEvent = null;
          continue;
        }

        if (line.startsWith("event:")) {
          const eventType = line.slice(6).trim();
          currentEvent = currentEvent ?? {};
          currentEvent.eventType = eventType;
          continue;
        }

        if (line.startsWith("data:")) {
          const dataLine = line.slice(5);
          currentEvent = currentEvent ?? {};
          currentEvent.data = currentEvent.data ? `${currentEvent.data}\n${dataLine}` : dataLine;
          continue;
        }

        if (line.startsWith("id:")) {
          continue;
        }
      }
    }
  }

  async function handleSseEvent(
    event: { eventType?: string; data?: string },
    subscribed: boolean,
    signal: AbortSignal,
  ): Promise<boolean> {
    if (!event.eventType) {
      return subscribed;
    }

    if (event.eventType === "PB_CONNECT" && event.data) {
      try {
        const parsed = JSON.parse(event.data) as { clientId?: string };
        if (typeof parsed?.clientId === "string") {
          await sendSubscription(parsed.clientId, signal);
        }
      } catch (error) {
        console.error("live/+page:handleSseEvent PB_CONNECT", { error });
      }
      return subscribed;
    }

    if (event.eventType === "PB_SUBSCRIBED") {
      return true;
    }

    if (event.eventType === "mob_hp_updates" && event.data) {
      try {
        const updates = parseMobHpUpdate(event.data);
        for (const update of updates) {
          upsertMobEntry(update.remote_id, update.server_id, update.hp_percent);
        }
      } catch (error) {
        console.error("live/+page:handleSseEvent mob_hp_updates", { error, payload: event.data });
      }
      return subscribed;
    }

    if (event.eventType === "mob_resets" && event.data) {
      const remoteId = event.data.replace(/^"+|"+$/g, "");
      if (remoteId && remoteId === activeRemoteId) {
        void ensureSeeded(remoteId, { force: true });
      }
      return subscribed;
    }

    return subscribed;
  }

  async function sendSubscription(clientId: string, signal: AbortSignal) {
    const response = await fetch(`${BPTIMER_BASE_URL}${REALTIME_ENDPOINT}`, {
      method: "POST",
      headers: {
        accept: "*/*",
        authorization: MOB_COLLECTION_AUTH_TOKEN,
        "content-type": "application/json",
      },
      body: JSON.stringify({
        clientId,
        subscriptions: ["mob_hp_updates", "mob_resets"],
      }),
      signal,
    });

    if (!response.ok) {
      const body = await response.text();
      throw new Error(`Subscription failed (${response.status}): ${body}`);
    }
  }

  function parseMobHpUpdate(raw: string): MobHpUpdate[] {
    const parsed = JSON.parse(raw);

    if (!Array.isArray(parsed)) {
      throw new Error("Unexpected mob_hp_updates payload format");
    }

    const records = parsed.length > 0 && Array.isArray(parsed[0]) ? parsed : [parsed];

    return records.map((record) => {
      if (!Array.isArray(record) || record.length < 3) {
        throw new Error("Unexpected mob_hp_updates payload format");
      }

      const [remoteId, serverId, hpPercent] = record;
      if (typeof remoteId !== "string") {
        throw new Error("mob_hp_updates payload missing remote id");
      }
      if (typeof serverId !== "number") {
        throw new Error("mob_hp_updates payload missing server id");
      }
      if (typeof hpPercent !== "number") {
        throw new Error("mob_hp_updates payload missing hp percent");
      }

      return {
        remote_id: remoteId,
        server_id: serverId,
        hp_percent: clampPercent(hpPercent),
      };
    });
  }

  function delay(ms: number, signal: AbortSignal) {
    return new Promise<void>((resolve, reject) => {
      const onAbort = () => {
        clearTimeout(timeoutId);
        signal.removeEventListener("abort", onAbort);
        reject(new DOMException("Aborted", "AbortError"));
      };

      const timeoutId = setTimeout(() => {
        signal.removeEventListener("abort", onAbort);
        resolve();
      }, ms);

      if (signal.aborted) {
        onAbort();
        return;
      }

      signal.addEventListener("abort", onAbort);
    });
  }

  async function loadMonsterOptions() {
    try {
      monsterOptions = await commandsExtended.getCrowdsourcedMonsterOptions();
    } catch (error) {
      console.error("live/+page:loadMonsterOptions", { error });
      monsterOptions = [];
    }
  }

  async function handleMonsterSelect(remoteId: string) {
    if (!remoteId || remoteId === currentMonster?.remote_id) {
      return;
    }

    try {
      const result = await commandsExtended.setCrowdsourcedMonsterRemote(remoteId);
      if (result.status === "error") {
        console.error("live/+page:setCrowdsourcedMonsterRemote", { error: result.error, remoteId });
        return;
      }

      switchActiveRemote(remoteId);
      mobHpStore.get(remoteId)?.clear();
      mobHpData = [];
      void ensureSeeded(remoteId, { force: true });
    } catch (error) {
      console.error("live/+page:setCrowdsourcedMonsterRemote", { error, remoteId });
    }
  }

  async function fetchData() {
    try {
      currentMonster = await commandsExtended.getCrowdsourcedMonster();
    } catch (error) {
      console.error("live/+page:getCrowdsourcedMonster", { error });
      currentMonster = null;
    }

    const remoteId = currentMonster?.remote_id ?? null;
    if (remoteId !== activeRemoteId) {
      switchActiveRemote(remoteId);
      if (remoteId) {
        void ensureSeeded(remoteId, { force: true });
      }
    }

    try {
      const lineResult = await commandsExtended.getLocalPlayerLine();
      currentLineId = lineResult.status === "ok" ? lineResult.data ?? null : null;
    } catch (error) {
      console.error("live/+page:getLocalPlayerLine", { error });
      currentLineId = null;
    }
  }

  onMount(() => {
    setStreamActive(true);
    void loadMonsterOptions();
    void fetchData();

    fetchInterval = setInterval(fetchData, 500);
    reseedInterval = setInterval(() => {
      if (activeRemoteId) {
        void ensureSeeded(activeRemoteId);
      }
    }, RESEED_INTERVAL_MS);
    cleanupInterval = setInterval(() => {
      cleanupMobHpStore();
      if (activeRemoteId) {
        refreshMobHpData();
      }
    }, 30_000);

    return () => {
      if (fetchInterval) clearInterval(fetchInterval);
      if (reseedInterval) clearInterval(reseedInterval);
      if (cleanupInterval) clearInterval(cleanupInterval);
      setStreamActive(false);
      mobHpStore.clear();
      mobHpLastChange.clear();
      mobHpData = [];
      activeRemoteId = null;
    };
  });
</script>

<div class="flex h-full w-full flex-col justify-start gap-2 p-1">
    {#if currentMonster}
      <div class="flex w-full flex-col gap-2">
        {#if mobHpData.length > 0}
          <div class="grid w-full max-h-[80px] gap-2 grid-cols-10 overflow-y-auto">
            {#each mobHpData
              .filter((mob) => {
                if (mob.hp_percent == 100 && mob.server_id < 20) { 
                  return false;
                }
                if(currentLineId === mob.server_id) {
                  return true;
                }
                return mob.hp_percent > 0;
              })
              .sort((a, b) => a.hp_percent - b.hp_percent ||  b.server_id - a.server_id )
              .slice(0, 200

              ) as mob}
              <div class={`relative overflow-hidden rounded-md border ${currentLineId === mob.server_id ? "border-primary/80 ring-2 ring-primary/30" : "border-neutral-700"} bg-neutral-900/60 p-2 text-center text-xs`}>
                <div
                  class={`absolute inset-y-0 left-0 ${barClass(mob.hp_percent)} transition-all duration-200`}
                  style={`width: ${clampPercent(mob.hp_percent)}%;`}
                ></div>
                <div class="relative z-10 flex flex-col items-center gap-0.5">
                  <span class="font-medium text-neutral-200">{mob.server_id}</span>
                  {#if currentLineId === mob.server_id}
                    <span class="rounded bg-primary/20 px-1 text-[0.65rem] uppercase tracking-wide text-primary"></span>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-xs text-neutral-600">No HP data available</p>
        {/if}
      </div>
    {:else}
      <p class="text-sm text-neutral-500">No timed monster found, select a monster to track:</p>
    {/if}
    <div class="flex w-full flex-col gap-1 md:w-1/2 lg:w-1/3">
      <select
        id="monster-select"
        class="w-full rounded-md border border-neutral-700 bg-neutral-900/60 px-2 py-1 text-sm text-neutral-200 outline-none transition-colors focus:border-neutral-500"
        disabled={monsterOptions.length === 0}
        value={selectedRemoteId ?? ""}
        onchange={(event) => handleMonsterSelect((event.currentTarget as HTMLSelectElement).value)}
      >
        <option value="" disabled selected={!selectedRemoteId}>
          {monsterOptions.length > 0 ? "Select monster" : "Loading monsters..."}
        </option>
        {#each monsterOptions as option}
          <option value={option.remote_id}>
            {option.name}
          </option>
        {/each}
      </select>
    </div>
</div>

