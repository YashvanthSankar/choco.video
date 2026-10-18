<script lang="ts">
  import { type RunResult } from "$lib/api";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import ClipCard from "./ClipCard.svelte";

  let { run = $bindable(), styles, presets, onNew }: { run: RunResult; styles: string[]; presets: string[]; onNew: () => void } = $props();
  const name = $derived(run.source.path.split("/").pop());
</script>

<div class="wrap">
  <div class="row head">
    <div class="grow">
      <span class="eyebrow">YOUR CLIPS</span><h1>{run.clips.length} {run.clips.length === 1 ? "clip" : "clips"} from {name}</h1>
      <p class="small">{run.picker.windows} moments considered · {run.picker.shortlisted} read by the picker · {run.picker.model_calls} model calls</p>
    </div>
    <button onclick={() => revealItemInDir(run.summary_path)}>Open folder</button>
    <button class="primary" onclick={onNew}>New video</button>
  </div>
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
  .head h1 { margin-top: 7px; overflow-wrap: anywhere; }
  .head p { margin: 0; }
  .eyebrow { font-size: 10px; font-weight: 750; letter-spacing: .13em; color: var(--accent); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(225px, 1fr)); gap: 15px; }
  .empty { text-align: center; padding: 52px 22px; }
  @media (max-width: 680px) { .wrap { padding: 26px 18px; } .head { flex-wrap: wrap; } }
</style>
