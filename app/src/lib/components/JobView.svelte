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
  let elapsed = $state(0);
  const started = Date.now();
  import { onMount } from "svelte";
  onMount(() => { const timer = setInterval(() => (elapsed = Math.floor((Date.now() - started) / 1000)), 1000); return () => clearInterval(timer); });
  const overall = $derived(Math.max(0, Math.min(100, ((Math.max(0, currentIdx) + Math.max(0, latest?.fraction ?? 0)) / STAGES.length) * 100)));
</script>

<div class="wrap">
  <span class="eyebrow">PROCESSING</span><h1>Finding your best clips</h1><p class="source" title={file}>{file.split("/").pop()}</p>
  <div class="card progress-card">
    <div class="progress-head"><strong>Overall progress</strong><span class="small muted tnum">{Math.round(overall)}% · {Math.floor(elapsed / 60)}:{String(elapsed % 60).padStart(2, '0')} elapsed</span></div>
    <div class="bar" role="progressbar" aria-label="Overall progress" aria-valuenow={Math.round(overall)} aria-valuemin="0" aria-valuemax="100"><div style={`width:${overall}%`}></div></div>
    <ol class="stages">
      {#each STAGES as s, i}
        <li class="stage" class:done={i < currentIdx} class:now={i === currentIdx}>
          <span class="stage-icon" aria-hidden="true">{#if i < currentIdx}<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3.5 8 3 3 6-6"/></svg>{:else if i === currentIdx}<span class="spinner"></span>{:else}<span class="pending"></span>{/if}</span>
          <div class="grow"><strong>{s.label}</strong>{#if i === currentIdx && latest}<p class="small detail" role="status">{latest.message}</p>{/if}</div>
          {#if i === currentIdx && latest && latest.fraction >= 0}<span class="small muted tnum">{Math.round(latest.fraction * 100)}%</span>{/if}
        </li>
      {/each}
    </ol>
  </div>
  <div class="row end"><span class="small muted">Processing stays on your Mac; only transcript text reaches your chosen provider.</span><button disabled={cancelling} onclick={() => { cancelling = true; onCancel(); }}>{cancelling ? "Cancelling…" : "Cancel job"}</button></div>
</div>
<style>
  .wrap { max-width: 670px; margin: 0 auto; padding: 54px 28px; }
  .eyebrow { font-size: 10px; font-weight: 750; letter-spacing: .13em; color: var(--accent); }
  h1 { margin-top: 8px; }
  .source { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; margin-bottom: 22px; }
  .progress-head { display: flex; justify-content: space-between; gap: 12px; align-items: baseline; margin-bottom: 11px; }
  .bar { height: 7px; border-radius: 20px; background: var(--panel-2); overflow: hidden; }
  .bar div { height: 100%; background: var(--caramel); border-radius: inherit; transition: width .3s ease; }
  .stages { margin: 22px 0 0; padding: 0; list-style: none; }
  .stage { display: flex; align-items: flex-start; gap: 12px; padding: 11px 0; color: var(--muted); border-top: 1px solid var(--line); }
  .stage strong { font-weight: 550; }
  .stage.now { color: var(--text); }
  .stage.now strong { font-weight: 700; }
  .stage.done { color: var(--good); }
  .stage-icon { display: grid; place-items: center; flex: 0 0 20px; height: 20px; }
  .stage-icon svg { width: 17px; height: 17px; }
  .pending { width: 9px; height: 9px; border: 1.5px solid var(--line); border-radius: 50%; }
  .spinner { width: 15px; height: 15px; border: 2px solid var(--line); border-top-color: var(--caramel); border-radius: 50%; animation: spin .85s linear infinite; }
  .detail { margin: 3px 0 0; color: var(--muted); font-weight: 400; }
  .end { margin-top: 16px; justify-content: space-between; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 620px) { .wrap { padding: 32px 18px; } }
</style>
