<script lang="ts">
  import { api, OPENCODE_URL, OPENCODE_SIGNUP, type Status, type Settings, type ProviderKind } from "$lib/api";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { untrack } from "svelte";

  let { status, settings = $bindable(), onKeySaved }: { status: Status; settings: Settings; onKeySaved?: () => void } = $props();

  let key = $state("");
  // Local copy: a child must not mutate its parent's props.
  let hasKey = $state<boolean>(untrack(() => status.has_key));
  let models = $state<string[]>([]);
  let busy = $state(false);
  let message = $state("");
  let ok = $state<boolean | null>(null);

  const appleReady = $derived(!!status.apple?.llm_available);
  const SIRI_SETTINGS = "x-apple.systempreferences:com.apple.Siri-Settings.extension";
  // Turn the helper's availability string into one instruction the user can act on.
  const appleHint = $derived.by((): { text: string; settings: boolean } => {
    const s = status.apple?.llm_status ?? "";
    const os = status.apple?.os ?? "";
    if (!status.apple || s.includes("needs macOS 26")) return { text: `Needs macOS 26 or later${os ? `; this Mac runs ${os}` : ""}.`, settings: false };
    if (s.includes("appleIntelligenceNotEnabled")) return { text: "Turn on Apple Intelligence in System Settings, then reopen choco.video.", settings: true };
    if (s.includes("modelNotReady")) return { text: "Apple Intelligence is still downloading its model. Try again in a few minutes.", settings: true };
    if (s.includes("deviceNotEligible")) return { text: "This Mac does not support Apple Intelligence. Use a key instead.", settings: false };
    return { text: "Apple's on-device model is not available on this Mac right now.", settings: false };
  });
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
      // Prove the key against the endpoint first; only a working key is saved to the Keychain.
      models = await api.providerModels(settings.provider.base_url, key.trim() || undefined);
      if (key.trim()) await api.setApiKey(key.trim());
      if (!settings.provider.score_model || !models.includes(settings.provider.score_model)) {
        settings.provider.score_model = ordered[0] ?? "";
      }
      if (!settings.provider.summary_model || !models.includes(settings.provider.summary_model)) {
        settings.provider.summary_model = settings.provider.score_model;
      }
      hasKey = true;
      onKeySaved?.();
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
  <button class="option" aria-pressed={settings.provider.kind === "opencode"} class:selected={settings.provider.kind === "opencode"} onclick={() => choose("opencode")}>
    <strong>OpenCode Zen</strong> <span class="pill">recommended</span>
    <span class="small muted">One key, many models. Pay a few cents per hour of video, directly to OpenCode.</span>
  </button>
  <button class="option" aria-pressed={settings.provider.kind === "custom"} class:selected={settings.provider.kind === "custom"} onclick={() => choose("custom")}>
    <strong>Another provider</strong>
    <span class="small muted">Any OpenAI-compatible endpoint: OpenRouter, Groq, DeepSeek, LM Studio, Ollama…</span>
  </button>
  <button class="option" aria-pressed={settings.provider.kind === "apple"} class:selected={settings.provider.kind === "apple"} disabled={!appleReady} onclick={() => choose("apple")}>
    <strong>Apple on-device</strong> <span class="pill">no key</span>
    <span class="small muted">
      {appleReady ? "Free and offline. Slower, and picks are weaker than a hosted model." : appleHint.text}
    </span>
  </button>
</div>
{#if !appleReady && appleHint.settings}
  <p class="small muted hint">
    No key yet? Apple's free picker works once Apple Intelligence is on.
    <button class="link" onclick={() => openUrl(SIRI_SETTINGS).catch(() => {})}>Open Apple Intelligence settings</button>
  </p>
{/if}

{#if settings.provider.kind !== "apple"}
  <div class="card setup">
    {#if settings.provider.kind === "custom"}
      <label for="base">Base URL</label>
      <input id="base" type="text" placeholder="https://api.example.com/v1" bind:value={settings.provider.base_url} />
    {/if}
    <label for="key">API key {#if hasKey}<span class="ok">· saved in your Keychain</span>{/if}</label>
    <div class="row">
      <input id="key" class="grow" type="password" placeholder={hasKey ? "Paste a new key to replace it" : "Paste your key"} bind:value={key} />
      <button class="primary" disabled={busy || (!key.trim() && !hasKey)} onclick={connect}>{hasKey && !key ? "Refresh models" : "Connect"}</button>
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
  .hint { margin: 10px 2px 0; }
  .link { background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; font: inherit; text-decoration: underline; }
  .options { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 9px; margin: 18px 0; }
  .option { display: flex; flex-direction: column; align-items: flex-start; gap: 7px; min-height: 132px; text-align: left; padding: 15px; border-radius: 10px; background: var(--panel);  }
  .option strong { font-size: 13px; }
  .option .pill { margin-top: -2px; }
  .option.selected { background: var(--hover); outline: 2px solid var(--text); outline-offset: -2px; }
  .option:disabled { opacity: .65; }
  .setup { margin-top: 4px; }
  .setup p { margin-top: 10px; }
  .models { align-items: flex-end; margin-bottom: 12px; }
  .msg { margin-top: 10px; }
  @media (max-width: 680px) { .options { grid-template-columns: 1fr; } .option { min-height: 0; } .models { flex-direction: column; align-items: stretch; } }
</style>
