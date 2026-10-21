<script lang="ts">
  import { words } from "$lib/platform.svelte";
  import { api, type Status, type Settings } from "$lib/api";
  import { open } from "@tauri-apps/plugin-dialog";
  import ProviderSetup from "./ProviderSetup.svelte";
  import { untrack } from "svelte";

  let { status, onClose }: { status: Status; onClose: (s: Settings) => void } = $props();
  let settings = $state<Settings>($state.snapshot(untrack(() => status.settings)));
  let saved = $state(false);
  let error = $state("");

  async function save() {
    error = "";
    try {
      await api.saveSettings(settings);
      saved = true;
      setTimeout(() => (saved = false), 1500);
    } catch (e) {
      error = String(e);
    }
  }
  async function pickDir() {
    const d = await open({ directory: true, defaultPath: settings.defaults.output_dir });
    if (typeof d === "string") settings.defaults.output_dir = d;
  }
  let keyRemoved = $state(false);
  async function removeKey() {
    await api.setApiKey("");
    keyRemoved = true;
  }
  let copied = $state("");
  async function copyDiagnostics() {
    try {
      const d = await api.diagnostics();
      await navigator.clipboard.writeText(JSON.stringify(d, null, 2));
      copied = "Copied. Paste it into your bug report.";
    } catch (e) {
      copied = `Could not copy: ${e}`;
    }
    setTimeout(() => (copied = ""), 4000);
  }
  let updateMsg = $state("");
  let updateVersion = $state("");
  let updating = $state(false);
  async function checkForUpdate() {
    updateMsg = "Checking…";
    updateVersion = "";
    try {
      const u = await api.checkUpdate();
      updateVersion = u?.version ?? "";
      updateMsg = u ? `choco.video ${u.version} is available.` : `You have the latest version (${status.app_version}).`;
    } catch (e) {
      updateMsg = `Could not check: ${e}`;
    }
  }
  async function installUpdate() {
    updating = true;
    updateMsg = "Downloading and installing. choco.video restarts when it is done.";
    try {
      await api.installUpdate();
    } catch (e) {
      updateMsg = `Update failed: ${e}`;
      updating = false;
    }
  }
  const appleSpeech = $derived(status.speech_engines.some((e) => e.kind === "apple_speech"));
</script>

<div class="wrap">
  <div class="row head"><div class="grow"><h1>Settings</h1><p class="small">Make choco.video work the way you do.</p></div><button onclick={() => onClose(settings)}>Done</button></div>

  <h2>Clip picker</h2>
  <ProviderSetup {status} bind:settings />
  {#if status.has_key && !keyRemoved}<button class="ghost small" onclick={removeKey}>Remove saved key</button>{/if}
  {#if keyRemoved}<p class="small muted">Key removed from the {words.vault}.</p>{/if}

  <h2 class="sp">Defaults</h2>
  <div class="card grid">
    <div><label for="ds">Captions</label><select id="ds" bind:value={settings.defaults.style}>{#each status.styles as s}<option value={s}>{s}</option>{/each}<option value="none">none</option></select></div>
    <div><label for="dp">Size</label><select id="dp" bind:value={settings.defaults.preset}>{#each status.presets as p}<option value={p}>{p}</option>{/each}</select></div>
    <div><label for="dl">Framing</label><select id="dl" bind:value={settings.defaults.layout}><option value="auto">Follow the speaker</option><option value="static">Centre crop</option><option value="fit">Whole frame</option></select></div>
    <div><label for="dn">Clips</label><input id="dn" type="number" min="1" max="30" bind:value={settings.defaults.max_clips} /></div>
    <div><label for="dm">Minimum score</label><input id="dm" type="number" min="0" max="100" bind:value={settings.defaults.min_score} /></div>
    <div class="wide"><label for="do">Save clips to</label><div class="row"><input id="do" class="grow" type="text" readonly value={settings.defaults.output_dir} /><button onclick={pickDir}>Choose…</button></div></div>
  </div>

  <h2 class="sp">Speech</h2>
  <div class="card grid">
    <div>
      <label for="sp">Engine</label>
      <select id="sp" bind:value={settings.speech}>
        <option value="auto">Automatic</option>
        {#if appleSpeech}<option value="apple">Apple speech (built in)</option>{/if}
        <option value="whisper">Whisper (downloaded model)</option>
      </select>
    </div>
    <div>
      <label for="lang">Language</label>
      <select id="lang" bind:value={settings.language}>
        <option value="">Automatic (English for Apple speech)</option>
        {#each status.apple?.locales ?? ["en-US"] as l}<option value={l}>{l}</option>{/each}
      </select>
    </div>
  </div>

  <h2 class="sp">Updates</h2>
  <div class="card">
    <label class="check"><input type="checkbox" bind:checked={settings.check_updates} /> Check for a new version when choco.video opens</label>
    <p class="small muted">Off unless you turn it on. Asks GitHub for the latest release; nothing about you or your videos is sent. Updates are signed and verified before they install.</p>
    <div class="row diag">
      <button class="small" onclick={checkForUpdate}>Check for updates</button>
      {#if updateVersion}<button class="small primary" disabled={updating} onclick={installUpdate}>Install and restart</button>{/if}
      <span class="small muted">{updateMsg}</span>
    </div>
  </div>

  <h2 class="sp">About</h2>
  <p class="small">choco.video engine {status.engine_version} · {status.hardware.gpu ?? status.hardware.arch} · {words.osName} {status.hardware.os_version}. Open source under the MIT license. Uses FFmpeg (LGPL 2.1), dav1d, whisper.cpp, YuNet, and the Inter and Montserrat fonts (SIL OFL).</p>
  <div class="row diag">
    <button class="small" onclick={copyDiagnostics}>Copy diagnostics</button>
    <span class="small muted">{copied || "For bug reports: hardware, encoders and settings. Never your key, videos or transcripts."}</span>
  </div>

  <div class="row end">
    {#if error}<span class="err small">{error}</span>{/if}
    {#if saved}<span class="ok small">Saved</span>{/if}
    <button class="primary" onclick={save}>Save</button>
  </div>
</div>

<style>
  .wrap { max-width: 870px; margin: 0 auto; padding: 38px 28px 48px; }
  .head { margin-bottom: 28px; }
  .head p { margin: 0; }
  h2 { padding-top: 28px; margin-bottom: 13px; }
  .sp { margin-top: 28px; }
  .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px 14px; }
  .wide { grid-column: 1 / -1; }
  .diag { gap: 12px; margin-top: 10px; }
  .check { display: flex; gap: 10px; align-items: center; font-weight: 500; }
  .end { justify-content: flex-end; margin-top: 24px; gap: 12px; }
  @media (max-width: 680px) { .wrap { padding: 25px 18px; } .grid { grid-template-columns: 1fr; } .wide { grid-column: auto; } }
</style>
