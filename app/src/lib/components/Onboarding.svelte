<script lang="ts">
  import { words } from "$lib/platform.svelte";
  import { api, onModelProgress, type Status, type Settings } from "$lib/api";
  import ProviderSetup from "./ProviderSetup.svelte";
  import { untrack } from "svelte";

  let { status, onDone }: { status: Status; onDone: (s: Settings) => void } = $props();

  let step = $state(0);
  // Onboarding edits a local copy of the settings, saved at the end.
  const initial = untrack(() => status);
  // $state.snapshot: a reactive proxy cannot be structuredClone'd (DataCloneError).
  let settings = $state<Settings>($state.snapshot(initial.settings));
  let downloading = $state(false);
  let dl = $state({ fraction: 0, message: "" });
  let dlError = $state("");
  let whisperInstalled = $state(initial.whisper_installed);

  const appleSpeech = $derived(status.speech_engines[0]?.kind === "apple_speech");
  // ProviderSetup proves a new key live and reports it; a key saved in an earlier session also counts.
  let keySaved = $state(untrack(() => status.has_key));
  const pickerReady = $derived(
    settings.provider.kind === "apple" ? !!status.apple?.llm_available : keySaved && !!settings.provider.score_model
  );
  const speechReady = $derived(appleSpeech || whisperInstalled);

  // Sensible first choice: Apple on-device if there is no key yet, and the recommended models.
  untrack(() => {
    if (!initial.has_key && initial.apple?.llm_available && !initial.settings.onboarded) settings.provider.kind = "apple";
    if (initial.recommended.length && !settings.provider.score_model) {
      settings.provider.score_model = initial.recommended[0].id;
      settings.provider.summary_model = initial.recommended[0].id;
    }
  });

  async function download() {
    downloading = true;
    dlError = "";
    const off = await onModelProgress((e) => (dl = { fraction: e.fraction, message: e.message }));
    try {
      await api.downloadSpeechModel();
      whisperInstalled = true;
    } catch (e) {
      dlError = String(e);
    } finally {
      off();
      downloading = false;
    }
  }

  async function finish() {
    settings.onboarded = true;
    await api.saveSettings(settings);
    onDone(settings);
  }
</script>

<div class="wrap"><div class="shell card">
  <div class="steps small muted" aria-label="Setup progress">
    {#each ["Welcome", "Clip picker", "Speech", "Ready"] as s, i}
      <span class:current={i === step} aria-current={i === step ? "step" : undefined}><b>{i + 1}</b>{s}</span>
    {/each}
  </div>

  {#if step === 0}
    <h1>Turn long videos into shorts</h1>
    <p>Drop in a podcast, talk or stream. choco.video finds the best moments, follows the speaker, adds captions and exports vertical clips ready for TikTok, Reels and Shorts.</p>
    <div class="card privacy">
      <strong>Your video never leaves this {words.machine}.</strong>
      <p class="small">Transcription, face tracking, captions and rendering all run here. Only the transcript text is sent to the clip picker you choose next, and only if you choose a hosted one.</p>
    </div>
    <div class="nav"><button class="primary" onclick={() => (step = 1)}>Set up the clip picker</button></div>
  {:else if step === 1}
    <h1>Choose the clip picker</h1>
    <p>A language model reads the transcript and scores which moments make good clips. Pick where it runs.</p>
    <ProviderSetup {status} bind:settings onKeySaved={() => (keySaved = true)} />
    <div class="nav">
      <button class="ghost" onclick={() => (step = 0)}>Back</button>
      <button class="primary" disabled={!pickerReady} onclick={() => (step = 2)}>Continue</button>
    </div>
  {:else if step === 2}
    <h1>Speech recognition</h1>
    {#if appleSpeech}
      <div class="card"><strong class="ok">Ready.</strong> <span class="muted">This {words.machine} transcribes with Apple's built-in speech engine. Nothing to download.</span></div>
    {:else if whisperInstalled}
      <div class="card"><strong class="ok">Ready.</strong> <span class="muted">The Whisper speech model is installed.</span></div>
    {:else}
      <div class="card">
        <p>This Mac needs a one-time speech model download (about 550 MB). It runs fully offline afterwards.</p>
        {#if downloading}
          <div class="bar"><div style={`width:${Math.max(2, dl.fraction * 100)}%`}></div></div>
          <p class="small">{dl.message}</p>
        {:else}
          <button class="primary" onclick={download}>Download speech model</button>
        {/if}
        {#if dlError}<p class="err small">{dlError}</p>{/if}
      </div>
    {/if}
    <div class="nav">
      <button class="ghost" onclick={() => (step = 1)}>Back</button>
      <button class="primary" disabled={!speechReady} onclick={() => (step = 3)}>Continue</button>
    </div>
  {:else}
    <h1>You're set</h1>
    <p>Clips are saved to <code>{settings.defaults.output_dir}</code>. You can change the picker, models and defaults any time in Settings.</p>
    <div class="nav"><button class="primary" onclick={finish}>Start clipping</button></div>
  {/if}
</div></div>

<style>
  .wrap { max-width: 820px; margin: 0 auto; padding: 42px 24px; }
  .shell { padding: 30px 34px; }
  .steps { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding-bottom: 16px; margin-bottom: 24px; }
  .steps span { display: inline-flex; align-items: center; gap: 7px; white-space: nowrap; }
  .steps b { display: inline-grid; place-items: center; width: 21px; height: 21px; background: var(--panel-2); border-radius: 50%; font-size: 11px; }
  .steps .current { color: var(--accent); font-weight: 650; }
  .steps .current b { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .privacy { margin: 23px 0; background: var(--panel-2); }
  .privacy p { margin: 5px 0 0; }
  .nav { display: flex; justify-content: flex-end; gap: 8px; margin-top: 28px; }
  .bar { height: 7px; background: var(--panel-2); border-radius: 9px; overflow: hidden; margin: 12px 0; }
  .bar div { height: 100%; background: var(--caramel); transition: width .3s; }
  code { overflow-wrap: anywhere; }
  @media (max-width: 660px) { .wrap { padding: 20px 14px; } .shell { padding: 20px; } .steps { flex-wrap: wrap; justify-content: flex-start; } }
</style>
