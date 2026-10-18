<script lang="ts">
  import { onMount } from "svelte";
  import { api, onJobDone, onJobError, onJobProgress, type JobRequest, type ProgressEvent, type RunResult, type Settings, type Status } from "$lib/api";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import Home from "$lib/components/Home.svelte";
  import JobView from "$lib/components/JobView.svelte";
  import Results from "$lib/components/Results.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";

  type View = "loading" | "onboarding" | "home" | "job" | "results" | "settings";
  let view = $state<View>("loading");
  let status = $state<Status | null>(null);
  let job = $state<number | null>(null);
  let jobFile = $state("");
  let events = $state<ProgressEvent[]>([]);
  let run = $state<RunResult | null>(null);
  let error = $state("");
  let fatal = $state("");

  async function refresh() {
    status = await api.status();
  }

  onMount(() => {
    const offs = [
      onJobProgress((e) => {
        if (e.job === job) events = [...events.slice(-200), e];
      }),
      onJobDone((e) => {
        if (e.job !== job) return;
        run = e.result;
        job = null;
        view = "results";
      }),
      onJobError((e) => {
        if (e.job !== job) return;
        job = null;
        error = e.error.includes("cancelled") ? "" : e.error;
        view = "home";
      }),
    ];
    refresh()
      .then(() => (view = status!.settings.onboarded ? "home" : "onboarding"))
      .catch((e) => (fatal = String(e)));
    return () => offs.forEach((p) => p.then((off) => off()));
  });

  async function start(req: JobRequest) {
    error = "";
    events = [];
    jobFile = req.file;
    try {
      job = await api.startJob(req);
      view = "job";
    } catch (e) {
      error = String(e);
    }
  }

  async function doneOnboarding(_: Settings) {
    await refresh();
    view = "home";
  }

  async function closeSettings(_: Settings) {
    await refresh();
    view = run ? "results" : "home";
  }
</script>

<header class="bar drag">
  <span class="brand">choco<span class="dot">.</span>video</span>
  {#if status && view !== "onboarding" && view !== "loading"}
    <nav class="nodrag">
      {#if view === "results"}<button class="ghost" onclick={() => (view = "home")}>New video</button>{/if}
      {#if view !== "job"}<button class="ghost" onclick={() => (view = "settings")}>Settings</button>{/if}
    </nav>
  {/if}
</header>

<main>
  {#if fatal}
    <div class="center"><p class="err">{fatal}</p></div>
  {:else if view === "loading" || !status}
    <div class="center"><p>Loading…</p></div>
  {:else if view === "onboarding"}
    <Onboarding {status} onDone={doneOnboarding} />
  {:else if view === "home"}
    <Home settings={status.settings} styles={status.styles} presets={status.presets} onStart={start} {error} />
  {:else if view === "job"}
    <JobView file={jobFile} {events} onCancel={() => job !== null && api.cancelJob(job)} />
  {:else if view === "results" && run}
    <Results bind:run styles={status.styles} presets={status.presets} onNew={() => (view = "home")} />
  {:else if view === "settings"}
    <SettingsView {status} onClose={closeSettings} />
  {/if}
</main>

<style>
  .bar { position: fixed; top: 0; left: 0; right: 0; height: 38px; display: flex; align-items: center; justify-content: space-between; padding: 0 14px 0 86px; background: rgba(21,17,14,.85); backdrop-filter: blur(10px); z-index: 10; border-bottom: 1px solid var(--line); }
  .brand { font-weight: 700; letter-spacing: -0.02em; }
  .dot { color: var(--accent); }
  nav { display: flex; gap: 6px; }
  nav button { padding: 4px 10px; font-size: 12px; }
  main { padding-top: 38px; min-height: 100vh; }
  .center { display: grid; place-items: center; height: 60vh; }
</style>
