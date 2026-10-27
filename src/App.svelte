<script lang="ts">
  import GridBenchmark from "./t0-6/GridBenchmark.svelte";

  const appName = "Picker";
</script>

<svelte:head>
  <title>{appName}</title>
</svelte:head>

<main>
  <h1>{appName}</h1>
  <GridBenchmark />
</main>
