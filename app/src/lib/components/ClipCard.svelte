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
      <button class="poster" onclick={() => (playing = true)} aria-label="Play">
        <img src={src(clip.poster)} alt={c.title} loading="lazy" />
        <span class="play">▶</span>
      </button>
    {/if}
  </div>
  <div class="meta">
    <div class="row top"><span class="pill score">{c.score}</span><span class="small muted">#{clip.rank} · {fmtTime(c.start_ms)}–{fmtTime(c.end_ms)} · {Math.round((c.end_ms - c.start_ms) / 1000)}s</span></div>
    <h3>{c.title}</h3>
    <p class="small">{c.description}</p>
    {#if c.reasons.length}<p class="small reasons">{c.reasons.join(" · ")}</p>{/if}
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
  .clip { padding: 0; overflow: hidden; display: flex; flex-direction: column; }
  .media { aspect-ratio: 9 / 16; background: #000; max-height: 420px; display: flex; justify-content: center; }
  video, img { width: 100%; height: 100%; object-fit: contain; display: block; }
  .poster { position: relative; width: 100%; height: 100%; padding: 0; border: 0; border-radius: 0; background: #000; }
  .play { position: absolute; inset: 0; display: grid; place-items: center; font-size: 34px; color: #fff; opacity: 0; transition: opacity .15s; background: rgba(0,0,0,.25); }
  .poster:hover .play { opacity: 1; }
  .meta { padding: 12px 14px 14px; }
  .top { gap: 8px; margin-bottom: 6px; }
  .score { background: var(--accent); color: var(--accent-ink); border-color: var(--accent); font-weight: 700; }
  h3 { font-size: 15px; margin-bottom: 4px; }
  .reasons { color: var(--muted); font-style: italic; }
  .actions { flex-wrap: wrap; gap: 6px; }
  .actions button { padding: 6px 10px; font-size: 12px; }
  .edit { display: grid; grid-template-columns: 1fr 1fr 1fr auto; gap: 6px; margin-top: 8px; }
  .edit select { padding: 6px; font-size: 12px; }
</style>
