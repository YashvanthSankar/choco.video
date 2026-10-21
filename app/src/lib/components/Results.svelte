<script lang="ts">
  import { type RunResult } from "$lib/api";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import ClipCard from "./ClipCard.svelte";

  let { run = $bindable(), styles, presets, onNew }: { run: RunResult; styles: string[]; presets: string[]; onNew: () => void } = $props();
  const name = $derived(run.source.path.split("/").pop());
  const dur = $derived(Math.max(1, run.source.duration_ms));
  const pct = (ms: number) => `${Math.min(100, (ms / dur) * 100)}%`;
  const mmss = (ms: number) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}`;
</script>

<div class="wrap">
  <div class="row head">
    <div class="grow">
      <h1>{run.clips.length} {run.clips.length === 1 ? "clip" : "clips"} ready</h1>
      <p class="small">From {name}: {run.picker.windows.toLocaleString()} moments considered, {run.picker.shortlisted} read by the picker, {run.picker.model_calls} model calls</p>
    </div>
    <button onclick={() => revealItemInDir(run.summary_path)}>Open folder</button>
    {#if run.fcpxml_path}<button onclick={() => revealItemInDir(run.fcpxml_path!)} title="Every clip as a project cut from the original video, for Final Cut Pro or DaVinci Resolve">Editor XML</button>{/if}
    <button class="primary" onclick={onNew}>New video</button>
  </div>
  {#if run.clips.length}
    <figure class="episode" aria-label="Where each clip sits in the episode">
      <div class="track">
        {#each run.clips as cl (cl.rank)}
          <span class="seg" style:left={pct(cl.candidate.start_ms)} style:width={`max(6px, ${pct(cl.candidate.end_ms - cl.candidate.start_ms)})`} title={`${cl.rank}. ${cl.candidate.title}`}><b>{cl.rank}</b></span>
        {/each}
      </div>
      <figcaption class="ticks tnum"><span>0:00</span><span>{mmss(dur)}</span></figcaption>
    </figure>
  {/if}
  {#if run.picker.errors.length}
    <p class="small muted">Notes: {run.picker.errors.slice(0, 3).join(" · ")}</p>
  {/if}
  {#if run.clips.length === 0}<div class="empty card"><h2>No clips found</h2><p>Try a different video or adjust your picker settings.</p><button class="primary" onclick={onNew}>Choose another video</button></div>{/if}
  <div class="grid">
    {#each run.clips as _, i (run.clips[i].rank)}
      <ClipCard bind:clip={run.clips[i]} summaryPath={run.summary_path} {styles} {presets} />
    {/each}
  </div>
</div>

<style>
  .wrap { max-width: 1220px; margin: 0 auto; padding: 38px 28px 48px; }
  .head { align-items: flex-start; margin-bottom: 24px; }
  .head h1 { overflow-wrap: anywhere; }
  .head p { margin: 0; }
  .episode { margin: 0 0 28px; }
  .track { position: relative; height: 40px; border-radius: 8px; background: var(--panel); }
  .seg { position: absolute; top: 6px; bottom: 6px; border-radius: 4px; background: var(--data); color: var(--invert-ink); display: grid; place-items: center; font-size: 12px; font-weight: 600; min-width: 18px; }
  .ticks { display: flex; justify-content: space-between; margin-top: 6px; font-size: 12px; color: var(--meta); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(225px, 1fr)); gap: 15px; }
  .empty { text-align: center; padding: 52px 22px; }
  @media (max-width: 680px) { .wrap { padding: 26px 18px; } .head { flex-wrap: wrap; } }
</style>
