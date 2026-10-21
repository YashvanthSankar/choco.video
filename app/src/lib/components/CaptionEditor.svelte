<script lang="ts">
  // Fix misheard caption words of one clip. Only word text changes; timings stay as spoken.
  import { onMount, tick } from "svelte";
  import { api, type CaptionWord, type ExportedClip } from "$lib/api";

  let { summaryPath, rank, layout, onSaved, onClose }: { summaryPath: string; rank: number; layout: string | null; onSaved: (clip: ExportedClip) => void; onClose: () => void } = $props();

  let groups = $state<CaptionWord[][] | null>(null);
  let edits = $state<Record<number, string>>({});
  let active = $state<number | null>(null);
  let draft = $state("");
  let wordError = $state("");
  let busy = $state(false);
  let error = $state("");
  let list = $state<HTMLElement>();

  const changed = $derived(Object.keys(edits).length);
  // The caption fonts have no emoji; the engine refuses them too, this just says so sooner.
  const EMOJI = /\p{Extended_Pictographic}/u;

  onMount(() => {
    api.clipWords(summaryPath, rank).then((g) => (groups = g), (e) => (error = String(e)));
  });

  async function begin(w: CaptionWord) {
    active = w.index;
    draft = edits[w.index] ?? w.text;
    wordError = "";
    await tick();
    list?.querySelector<HTMLInputElement>("input")?.select();
  }
  async function finish(index: number, refocus: boolean) {
    active = null;
    wordError = "";
    if (!refocus) return;
    await tick();
    list?.querySelector<HTMLButtonElement>(`[data-i="${index}"]`)?.focus();
  }
  function commit(w: CaptionWord, refocus: boolean) {
    if (active !== w.index) return;
    const text = draft.trim();
    if (!text) return void (wordError = "A word can't be empty. Type a replacement.");
    if (/[\r\n]/.test(text)) return void (wordError = "A word can't contain a line break.");
    if (EMOJI.test(text)) return void (wordError = "Emoji can't be burned into captions yet: the caption fonts have none.");
    if (text === w.text) delete edits[w.index];
    else edits[w.index] = text;
    finish(w.index, refocus);
  }
  function onKey(e: KeyboardEvent, w: CaptionWord) {
    if (e.key === "Enter") {
      e.preventDefault();
      commit(w, true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      finish(w.index, true);
    }
  }
  async function save() {
    busy = true;
    error = "";
    try {
      const out = Object.entries(edits).map(([i, text]) => ({ index: Number(i), text }));
      onSaved(await api.updateCaptionWords(summaryPath, rank, out, layout));
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="captions" aria-label={`Captions of clip ${rank}`}>
  {#if groups}
    <p class="small muted" id={`caption-hint-${rank}`}>Select a word to fix it. Enter keeps the change, Escape cancels.</p>
    <div class="words" bind:this={list}>
      {#each groups as g}
        <p class="line">
          {#each g as w (w.index)}
            {#if active === w.index}
              <input type="text" class="word-input" bind:value={draft} size={Math.max(3, draft.length + 1)} aria-label={`Word ${w.index + 1}, heard as “${w.text}”`} aria-describedby={`caption-hint-${rank}`} aria-invalid={wordError !== ""} onkeydown={(e) => onKey(e, w)} onblur={() => commit(w, false)} />
            {:else}
              <button class="word" class:changed={w.index in edits} data-i={w.index} title={w.index in edits ? `Heard as “${w.text}”` : undefined} onclick={() => begin(w)}>{edits[w.index] ?? w.text}</button>
            {/if}
          {/each}
        </p>
      {:else}
        <p class="small muted">No words in this clip.</p>
      {/each}
    </div>
    {#if wordError}<p class="err small" role="alert">{wordError}</p>{/if}
    <div class="row foot">
      <span class="small muted grow" aria-live="polite">{changed} {changed === 1 ? "change" : "changes"}</span>
      <button class="ghost" disabled={busy} onclick={onClose}>Cancel</button>
      <button class="primary" disabled={busy || changed === 0} onclick={save}>{busy ? "Rendering…" : "Save"}</button>
    </div>
  {:else if !error}
    <p class="small muted" role="status">Loading words…</p>
  {/if}
  {#if error}<p class="err small" role="alert">{error}</p>{/if}
</section>

<style>
  .captions { margin-top: 11px; }
  .captions > p { margin-bottom: 7px; }
  .words { max-height: 240px; overflow-y: auto; background: var(--panel-2); border-radius: 8px; padding: 7px 5px; }
  .line { margin: 0 0 5px; color: var(--text); font-size: 13px; line-height: 1.75; }
  .line:last-child { margin-bottom: 0; }
  .word { min-height: 0; padding: 1px 3px; border-radius: 4px; background: transparent; font-weight: 400; }
  .word:hover:not(:disabled) { background: var(--hover); }
  .word.changed { background: var(--pressed); text-decoration: underline; text-underline-offset: 3px; }
  .word-input { width: auto; min-height: 0; padding: 1px 4px; border-radius: 4px; background: var(--bg); font-size: 13px; }
  .foot { gap: 5px; margin-top: 9px; }
  .foot button { padding: 5px 10px; min-height: 29px; font-size: 12px; }
</style>
