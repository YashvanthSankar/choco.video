<script lang="ts">
  import { type Settings, type JobRequest } from "$lib/api";
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount, untrack } from "svelte";

  let { settings, styles, presets, onStart, error = "" }: { settings: Settings; styles: string[]; presets: string[]; onStart: (r: JobRequest) => void; error?: string } = $props();

  let file = $state("");
  let prompt = $state("");
  // The form starts from the saved defaults and is then edited locally.
  const d = untrack(() => settings.defaults);
  let style = $state(d.style);
  let preset = $state(d.preset);
  let layout = $state(d.layout);
  let maxClips = $state(d.max_clips);
  let outputDir = $state(d.output_dir);
  let hovering = $state(false);
  const VIDEO = ["mp4", "mov", "m4v", "mkv", "webm", "avi", "m4a", "mp3", "wav"];

  onMount(() => {
    const p = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "over") hovering = true;
      else if (e.payload.type === "drop") {
        hovering = false;
        const f = e.payload.paths.find((p) => VIDEO.includes(p.split(".").pop()?.toLowerCase() ?? ""));
        if (f) file = f;
      } else hovering = false;
    });
    return () => { p.then((off) => off()); };
  });

  async function pick() {
    const f = await open({ multiple: false, filters: [{ name: "Video or audio", extensions: VIDEO }] });
    if (typeof f === "string") file = f;
  }
  async function pickDir() {
    const d = await open({ directory: true, defaultPath: outputDir });
    if (typeof d === "string") outputDir = d;
  }
  function start() {
    onStart({ file, prompt: prompt.trim() || null, style, preset, layout, max_clips: maxClips, min_score: settings.defaults.min_score, output_dir: outputDir });
  }
  const name = $derived(file.split("/").pop() ?? "");
</script>

<div class="wrap">
  <button class="drop" class:hovering class:has={!!file} onclick={pick}>
    {#if file}
      <strong>{name}</strong>
      <span class="small muted">Click to choose a different file</span>
    {:else}
      <strong>Drop a long video here</strong>
      <span class="small muted">or click to choose · MP4, MOV, MKV, WebM, and more</span>
    {/if}
  </button>

  <div class="card opts">
    <label for="prompt">Only clips about… (optional)</label>
    <input id="prompt" type="text" placeholder="e.g. pricing, the founding story, AI" bind:value={prompt} />
    <div class="grid">
      <div>
        <label for="n">Up to</label>
        <select id="n" bind:value={maxClips}>
          {#each [3, 5, 8, 10, 15, 20] as n}<option value={n}>{n} clips</option>{/each}
        </select>
      </div>
      <div>
        <label for="style">Captions</label>
        <select id="style" bind:value={style}>
          {#each styles as s}<option value={s}>{s}</option>{/each}
          <option value="none">none (subtitle files only)</option>
        </select>
      </div>
      <div>
        <label for="preset">Size</label>
        <select id="preset" bind:value={preset}>
          {#each presets as p}<option value={p}>{p === "shorts" ? "9:16 vertical" : p === "square" ? "1:1 square" : p === "portrait" ? "4:5 portrait" : "16:9 landscape"}</option>{/each}
        </select>
      </div>
      <div>
        <label for="layout">Framing</label>
        <select id="layout" bind:value={layout}>
          <option value="auto">Follow the speaker</option>
          <option value="static">Centre crop</option>
          <option value="fit">Whole frame, blurred fill</option>
        </select>
      </div>
    </div>
    <label for="out">Save clips to</label>
    <div class="row">
      <input id="out" class="grow" type="text" readonly value={outputDir} />
      <button onclick={pickDir}>Choose…</button>
    </div>
  </div>

  {#if error}<p class="err">{error}</p>{/if}
  <div class="row end"><button class="primary big" disabled={!file} onclick={start}>Find clips</button></div>
</div>

<style>
  .wrap { max-width: 820px; margin: 0 auto; padding: 56px 28px 28px; }
  .drop { width: 100%; height: 190px; display: flex; flex-direction: column; justify-content: center; align-items: center; gap: 6px; border: 2px dashed var(--line); border-radius: 16px; background: var(--panel); font-size: 16px; }
  .drop.hovering, .drop:hover { border-color: var(--accent); }
  .drop.has { border-style: solid; }
  .opts { margin-top: 16px; }
  .grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; }
  .end { justify-content: flex-end; margin-top: 18px; }
  .big { padding: 11px 26px; font-size: 15px; }
</style>
