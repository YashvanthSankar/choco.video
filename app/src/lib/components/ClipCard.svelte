<script lang="ts">
  import { api, fmtTime, type ExportedClip } from "$lib/api";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";

  let { clip = $bindable(), summaryPath, styles, presets }: { clip: ExportedClip; summaryPath: string; styles: string[]; presets: string[] } = $props();

  let playing = $state(false);
  let editing = $state(false);
  let busy = $state(false);
  let error = $state("");
  let copied = $state(false);
  let style = $state("bold");
  let preset = $state("shorts");
  let layout = $state("auto");
  let bust = $state(0);

  const c = $derived(clip.candidate);
  const src = (p: string) => `${convertFileSrc(p)}?v=${bust}`;

  async function reexport() {
    busy = true;
    error = "";
    try {
      clip = await api.reexportClip(summaryPath, clip.rank, style, preset, layout);
      bust++;
      editing = false;
      playing = false;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function copyCaption() {
    await navigator.clipboard.writeText(`${c.title}\n\n${c.description}\n\n${c.hashtags.join(" ")}`);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="clip card">
  <div class="media">
    {#if playing}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video src={src(clip.mp4)} controls autoplay></video>
    {:else}
      <button class="poster" onmouseenter={() => (playing = true)} onclick={() => (playing = true)} aria-label={`Play clip ${clip.rank}: ${c.title}`}>
        <img src={src(clip.poster)} alt={c.title} loading="lazy" />
        <span class="play" aria-hidden="true"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.8c0-.8.9-1.3 1.6-.9l9.1 6.2a1.1 1.1 0 0 1 0 1.8l-9.1 6.2c-.7.4-1.6-.1-1.6-.9V5.8Z"/></svg></span>
      </button>
    {/if}
  </div>
  <div class="meta">
    <div class="row top"><span class="score tnum"><b>{c.score}</b> score · #{clip.rank}</span><span class="small muted tnum">{fmtTime(c.start_ms)}–{fmtTime(c.end_ms)} · {Math.round((c.end_ms - c.start_ms) / 1000)}s</span></div>
    <h3>{c.title}</h3>
    <p class="small">{c.description}</p>
    {#if c.reasons.length}<p class="small reasons">Why: {c.reasons[0]}</p>{/if}
    <div class="row actions">
      <button onclick={() => revealItemInDir(clip.mp4)}>Show in Finder</button>
      <button onclick={copyCaption}>{copied ? "Copied" : "Copy caption"}</button>
      <button class="ghost" onclick={() => (editing = !editing)}>Restyle</button>
    </div>
    {#if editing}
      <div class="edit">
        <select bind:value={style}>{#each styles as s}<option value={s}>{s}</option>{/each}<option value="none">no captions</option></select>
        <select bind:value={preset}>{#each presets as p}<option value={p}>{p}</option>{/each}</select>
        <select bind:value={layout}><option value="auto">follow speaker</option><option value="static">centre</option><option value="fit">fit</option></select>
        <button class="primary" disabled={busy} onclick={reexport}>{busy ? "Rendering…" : "Render"}</button>
      </div>
    {/if}
    {#if error}<p class="err small">{error}</p>{/if}
  </div>
</div>

<style>
  .clip { padding: 0; overflow: hidden; display: flex; flex-direction: column; min-width: 0; }
  .media { position: relative; aspect-ratio: 9 / 16; background: var(--ink); display: flex; justify-content: center; overflow: hidden; }
  video, img { width: 100%; height: 100%; object-fit: contain; display: block; }
  .poster { position: relative; width: 100%; height: 100%; padding: 0; border: 0; border-radius: 0; background: var(--ink); }
  .play { position: absolute; inset: 0; display: grid; place-items: center; color: var(--play-ink); background: var(--play-scrim); transition: background .16s; }
  .play svg { width: 42px; height: 42px; padding: 11px; background: var(--play-control); border-radius: 50%; }
  .poster:hover .play { background: var(--play-scrim-hover); }
  .meta { padding: 15px; display: flex; flex-direction: column; flex: 1; }
  .top { gap: 8px; justify-content: space-between; margin-bottom: 9px; }
  .top .small { white-space: nowrap; }
  .score { font-size: 13px; color: var(--muted); }
  .score b { color: var(--data); font-weight: 600; font-size: 15px; }
  h3 { font-size: 14px; line-height: 1.3; margin-bottom: 5px; }
  p { margin-bottom: 9px; }
  .reasons { color: var(--meta); }
  .actions { flex-wrap: wrap; gap: 5px; margin-top: auto; padding-top: 8px; }
  .actions button { padding: 5px 7px; min-height: 29px; font-size: 11px; }
  .edit { display: grid; grid-template-columns: 1fr 1fr; gap: 7px; margin-top: 11px; }
  .edit select { padding: 6px; font-size: 12px; }
  .edit button { grid-column: 1 / -1; }
</style>
