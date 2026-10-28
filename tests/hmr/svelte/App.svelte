<script>
  import Child from './Child.svelte';
  let count = $state(0);
</script>

<button onclick={() => count++}>count {count}</button>
<span id="label">Hello</span>
<Child />

<style>
  span { color: rgb(1, 2, 3); }
</style>
