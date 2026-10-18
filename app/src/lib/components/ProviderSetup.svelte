<script lang="ts">
  import { api, OPENCODE_URL, OPENCODE_SIGNUP, type Status, type Settings, type ProviderKind } from "$lib/api";
  import { openUrl } from "@tauri-apps/plugin-opener";

  let { status, settings = $bindable() }: { status: Status; settings: Settings } = $props();

  let key = $state("");
  let models = $state<string[]>([]);
  let busy = $state(false);
  let message = $state("");
  let ok = $state<boolean | null>(null);

  const appleReady = $derived(!!status.apple?.llm_available);
  const rec = $derived(status.recommended);
  const recIds = $derived(new Set(rec.map((r) => r.id)));
  const ordered = $derived([...rec.map((r) => r.id).filter((id) => models.length === 0 || models.includes(id)), ...models.filter((m) => !recIds.has(m))]);

  function choose(kind: ProviderKind) {
    settings.provider.kind = kind;
    message = "";
    ok = null;
    if (kind === "opencode") settings.provider.base_url = OPENCODE_URL;
    if (kind !== "apple" && !settings.provider.score_model && rec.length) {
      settings.provider.score_model = rec[0].id;
      settings.provider.summary_model = rec[0].id;
    }
  }

  async function connect() {
    busy = true;
    message = "";
    ok = null;
    try {
      if (key.trim()) await api.setApiKey(key.trim());
      models = await api.providerModels(settings.provider.base_url, key.trim() || undefined);
      if (!settings.provider.score_model || !models.includes(settings.provider.score_model)) {
        settings.provider.score_model = ordered[0] ?? "";
      }
      if (!settings.provider.summary_model || !models.includes(settings.provider.summary_model)) {
        settings.provider.summary_model = settings.provider.score_model;
      }
      status.has_key = true;
      key = "";
      message = `Connected. ${models.length} models available.`;
      ok = true;
    } catch (e) {
      message = String(e);
      ok = false;
    } finally {
      busy = false;
    }
  }

  async function test() {
    busy = true;
    message = "Sending one tiny scoring request…";
    ok = null;
    try {
      const secs = await api.providerTest(settings.provider.base_url, settings.provider.score_model);
      message = `${settings.provider.score_model} returned valid scores in ${secs.toFixed(1)} s.`;
      ok = true;
    } catch (e) {
      message = String(e);
      ok = false;
    } finally {
      busy = false;
    }
  }

  const label = (id: string) => {
    const r = rec.find((x) => x.id === id);
    return r ? `${r.label} · ~${r.cents_per_source_hour.toFixed(1)}¢ per hour of video` : id;
  };
</script>

<div class="options">
  <button class="option" class:selected={settings.provider.kind === "opencode"} onclick={() => choose("opencode")}>
    <strong>OpenCode Zen</strong> <span class="pill">recommended</span>
    <span class="small muted">One key, many models. Pay a few cents per hour of video, directly to OpenCode.</span>
  </button>
  <button class="option" class:selected={settings.provider.kind === "custom"} onclick={() => choose("custom")}>
    <strong>Another provider</strong>
    <span class="small muted">Any OpenAI-compatible endpoint: OpenRouter, Groq, DeepSeek, LM Studio, Ollama…</span>
  </button>
  <button class="option" class:selected={settings.provider.kind === "apple"} disabled={!appleReady} onclick={() => choose("apple")}>
    <strong>Apple on-device</strong> <span class="pill">no key</span>
    <span class="small muted">
      {appleReady ? "Free and offline. Slower, and picks are weaker than a hosted model." : "Needs macOS 26 with Apple Intelligence turned on."}
    </span>
  </button>
</div>

{#if settings.provider.kind !== "apple"}
  <div class="card setup">
    {#if settings.provider.kind === "custom"}
      <label for="base">Base URL</label>
      <input id="base" type="text" placeholder="https://api.example.com/v1" bind:value={settings.provider.base_url} />
    {/if}
    <label for="key">API key {#if status.has_key}<span class="ok">· saved in your Keychain</span>{/if}</label>
    <div class="row">
      <input id="key" class="grow" type="password" placeholder={status.has_key ? "Paste a new key to replace it" : "Paste your key"} bind:value={key} />
      <button class="primary" disabled={busy || (!key.trim() && !status.has_key)} onclick={connect}>{status.has_key && !key ? "Refresh models" : "Connect"}</button>
    </div>
    {#if settings.provider.kind === "opencode"}
      <p class="small">No key yet? <a href={OPENCODE_SIGNUP} onclick={(e) => { e.preventDefault(); openUrl(OPENCODE_SIGNUP); }}>Create one on OpenCode Zen</a>, add a little credit, paste it here. It never leaves this Mac except to call OpenCode.</p>
    {/if}

    {#if models.length || settings.provider.score_model}
      <div class="row models">
        <div class="grow">
          <label for="score">Scoring model (many small calls)</label>
          <select id="score" bind:value={settings.provider.score_model}>
            {#each ordered.length ? ordered : [settings.provider.score_model] as m}
              <option value={m}>{label(m)}</option>
            {/each}
          </select>
        </div>
        <div class="grow">
          <label for="summary">Summary model (one call per video)</label>
          <select id="summary" bind:value={settings.provider.summary_model}>
            {#each ordered.length ? ordered : [settings.provider.summary_model] as m}
              <option value={m}>{label(m)}</option>
            {/each}
          </select>
        </div>
      </div>
      <div class="row">
        <button disabled={busy || !settings.provider.score_model} onclick={test}>Test connection</button>
        {#if rec[0] && settings.provider.score_model === rec[0].id}<span class="small muted">{rec[0].note}</span>{/if}
      </div>
    {/if}
  </div>
{/if}

{#if message}
  <p class="msg" class:ok={ok === true} class:err={ok === false}>{message}</p>
{/if}

<style>
  .options { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin: 14px 0; }
  .option { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; text-align: left; padding: 14px; border-radius: var(--radius); background: var(--panel); }
  .option.selected { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }
  .setup { margin-top: 4px; }
  .models { align-items: flex-end; margin-bottom: 12px; }
  .msg { margin-top: 10px; }
</style>
