<script setup>
import { ref } from 'vue';
import Child from './Child.vue';
const count = ref(0);
</script>

<template>
  <div>
    <button @click="count++">count {{ count }}</button>
    <span id="label">Hello</span>
    <Child />
  </div>
</template>

<style scoped>
span { color: rgb(1, 2, 3); }
</style>
