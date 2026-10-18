<script lang="ts">
  import { type ProgressEvent } from "$lib/api";

  let { file, events, onCancel }: { file: string; events: ProgressEvent[]; onCancel: () => void } = $props();

  const STAGES = [
    { key: "ingest", label: "Reading audio" },
    { key: "transcribe", label: "Transcribing" },
    { key: "highlights", label: "Finding the best moments" },
    { key: "render", label: "Rendering clips" },
    { key: "export", label: "Exporting" },
  ];
  const latest = $derived(events.at(-1));
  const reached = $derived(new Set(events.map((e) => e.stage)));
  const currentIdx = $derived(Math.max(...STAGES.map((s, i) => (reached.has(s.key) ? i : -1))));
  let cancelling = $state(false);
</script>

<div class="wrap">
  <h1>Working on {file.split("/").pop()}</h1>
  <p class="small">Everything here runs on this Mac except the clip picker call. You can keep using your computer.</p>
  <div class="card">
    {#each STAGES as s, i}
      <div class="stage" class:done={i < currentIdx} class:now={i === currentIdx}>
        <span class="dot"></span>
        <span class="grow">{s.label}</span>
        {#if i === currentIdx && latest && latest.fraction >= 0}<span class="small muted">{Math.round(latest.fraction * 100)}%</span>{/if}
      </div>
      {#if i === currentIdx && latest}
        <div class="bar"><div style={`width:${Math.max(3, (latest.fraction >= 0 ? latest.fraction : 0.5) * 100)}%`} class:indeterminate={latest.fraction < 0}></div></div>
        <p class="small msg">{latest.message}</p>
      {/if}
    {/each}
  </div>
  <div class="row end">
    <button disabled={cancelling} onclick={() => { cancelling = true; onCancel(); }}>{cancelling ? "Cancelling…" : "Cancel"}</button>
  </div>
</div>

<style>
  .wrap { max-width: 720px; margin: 0 auto; padding: 56px 28px 28px; }
  .stage { display: flex; gap: 10px; align-items: center; padding: 8px 0; color: var(--muted); }
  .stage.now { color: var(--text); font-weight: 600; }
  .stage.done { color: var(--good); }
  .dot { width: 9px; height: 9px; border-radius: 9px; background: var(--line); }
  .stage.now .dot { background: var(--accent); }
  .stage.done .dot { background: var(--good); }
  .bar { height: 6px; background: var(--panel-2); border-radius: 9px; overflow: hidden; margin: 2px 0 6px 19px; }
  .bar div { height: 100%; background: var(--accent); transition: width .3s; }
  .bar div.indeterminate { animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: .4; } }
  .msg { margin-left: 19px; }
  .end { justify-content: flex-end; margin-top: 16px; }
</style>
