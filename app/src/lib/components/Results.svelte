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
      <h1>{run.clips.length} clips from {name}</h1>
      <p class="small">{run.picker.windows} moments considered · {run.picker.shortlisted} read by the picker · {run.picker.model_calls} model calls</p>
    </div>
    <button onclick={() => revealItemInDir(run.summary_path)}>Open folder</button>
    <button class="primary" onclick={onNew}>New video</button>
  </div>
  {#if run.picker.errors.length}
    <p class="small muted">Notes: {run.picker.errors.slice(0, 3).join(" · ")}</p>
  {/if}
  <div class="grid">
    {#each run.clips as _, i (run.clips[i].rank)}
      <ClipCard bind:clip={run.clips[i]} summaryPath={run.summary_path} {styles} {presets} />
    {/each}
  </div>
</div>

<style>
  .wrap { padding: 48px 28px 28px; }
  .head { align-items: flex-start; margin-bottom: 14px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 16px; }
</style>
