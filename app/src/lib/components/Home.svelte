<script lang="ts">
  import { words } from "$lib/platform.svelte";
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
  // Keep a saved value that is not one of the presets selectable instead of showing a blank select.
  const clipCounts = [...new Set([3, 5, 8, 10, 15, 20, d.max_clips])].sort((x, y) => x - y);
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
  <div class="intro"><h1>Find the moments worth sharing.</h1><p>Turn one long recording into polished, ready-to-post clips.</p></div>
  <button class="drop" class:hovering class:has={!!file} onclick={pick} aria-label={file ? `Change source file, currently ${name}` : 'Choose a video or audio file'}>
    <span class="drop-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="3"/><path d="m10 9 5 3-5 3z"/></svg></span>
    {#if file}
      <strong title={name}>{name}</strong><span class="small muted">Source selected · Click to choose another</span>
    {:else}
      <strong>Drop a video or audio file here</strong><span class="small muted">or click to browse · MP4, MOV, MKV, WebM, MP3 and more</span>
    {/if}
  </button>
  <div class="card opts">
    <div class="section-head"><div><h2>Clip settings</h2><p class="small">A few choices before we find your highlights.</p></div></div>
    <label for="prompt">Focus on a topic <span class="muted optional">Optional</span></label>
    <input id="prompt" type="text" placeholder="e.g. pricing, the founding story, AI" bind:value={prompt} />
    <div class="grid">
      <div><label for="n">Number of clips</label><select id="n" bind:value={maxClips}>{#each clipCounts as n}<option value={n}>Up to {n}</option>{/each}</select></div>
      <div><span class="field-label">Captions</span><div class="segments" role="group" aria-label="Caption style">{#each [...styles, 'none'] as s}<button class:active={style === s} aria-pressed={style === s} onclick={() => (style = s)}>{s === 'none' ? 'Off' : s}</button>{/each}</div></div>
      <div><span class="field-label">Output size</span><div class="segments" role="group" aria-label="Output size">{#each presets as p}<button class:active={preset === p} aria-pressed={preset === p} title={p} onclick={() => (preset = p)}>{p === 'shorts' ? '9:16' : p === 'square' ? '1:1' : p === 'portrait' ? '4:5' : '16:9'}</button>{/each}</div></div>
      <div><span class="field-label">Framing</span><div class="segments" role="group" aria-label="Framing">{#each [{value:'auto',label:'Follow'}, {value:'static',label:'Centre'}, {value:'fit',label:'Fit'}] as choice}<button class:active={layout === choice.value} aria-pressed={layout === choice.value} title={choice.value === 'auto' ? 'Follow the speaker' : choice.value === 'static' ? 'Centre crop' : 'Whole frame, blurred fill'} onclick={() => (layout = choice.value)}>{choice.label}</button>{/each}</div></div>
    </div>
    <label for="out">Save clips to</label><div class="row destination"><input id="out" class="grow" type="text" readonly value={outputDir} /><button onclick={pickDir}>Choose…</button></div>
  </div>
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  <div class="row end"><span class="small muted">Your video stays on this {words.machine}.</span><button class="primary big" disabled={!file} title={!file ? 'Choose a source file to find clips' : 'Find clips'} onclick={start}>Find clips <span aria-hidden="true">→</span></button></div>
</div>
<style>
  .wrap { max-width: 850px; margin: 0 auto; padding: 42px 28px 40px; }
  .intro { margin-bottom: 22px; }
  .intro p { margin-bottom: 0; }
  .drop { width: 100%; min-height: 166px; display: flex; flex-direction: column; justify-content: center; align-items: center; gap: 5px; border: 1.5px dashed var(--hover); border-radius: 12px; background: var(--panel); text-align: center; padding: 22px; }
  .drop:hover, .drop.hovering { border-color: var(--caramel); background: var(--panel-2); }
  .drop.hovering { background: var(--panel-2); }
  .drop.has { border-style: solid; }
  .drop strong { font-size: 15px; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .drop-icon { width: 40px; height: 40px; display: grid; place-items: center; color: var(--accent); background: var(--panel-2); border-radius: 10px; margin-bottom: 6px; }
  .drop-icon svg { width: 22px; height: 22px; }
  .opts { margin-top: 16px; }
  .section-head { padding-bottom: 4px; }
  .section-head h2, .section-head p { margin-bottom: 0; }
  .optional { font-weight: 400; margin-left: 4px; }
  .grid { display: grid; grid-template-columns: 1fr 2fr; gap: 2px 18px; }
  .field-label { display: block; font-size: 12px; font-weight: 600; margin: 14px 0 6px; }
  .segments { display: flex; width: 100%; padding: 3px; border-radius: 8px; background: var(--panel-2); min-height: 34px; }
  .segments button { flex: 1; min-width: 0; min-height: 26px; padding: 3px 7px; border: 0; background: transparent; color: var(--muted); font-size: 12px; text-transform: capitalize; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .segments button:hover:not(:disabled) { background: var(--hover); border: 0; }
  .segments button.active { background: var(--hover); color: var(--text); font-weight: 600; }
  .destination { flex-wrap: nowrap; }
  .end { justify-content: space-between; margin-top: 18px; }
  .big { min-width: 132px; min-height: 36px; }
  @media (max-width: 650px) { .wrap { padding: 28px 18px; } .grid { grid-template-columns: 1fr; } .end { flex-wrap: wrap; } }
</style>
